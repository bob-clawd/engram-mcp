use anyhow::Result;
use rmcp::schemars::JsonSchema;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

use crate::{
    memory::{MemoryService, RecallMemory, RetentionTier},
    memory_store::JsonMemoryStore,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecallResponse {
    memories: Vec<RecallMemory>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct RememberRequest {
    text: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MemoryIdsRequest {
    memory_ids: Vec<String>,
}

#[derive(Clone)]
pub struct EngramServer {
    memory_service: MemoryService<JsonMemoryStore>,
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Self>,
}

impl EngramServer {
    pub fn new(memory_service: MemoryService<JsonMemoryStore>) -> Self {
        Self {
            memory_service,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl EngramServer {
    #[tool(
        name = "recall",
        description = "Load the strongest current memories. Useful at the start of a session. Returns up to 50 memories."
    )]
    async fn recall(&self) -> Result<CallToolResult, ErrorData> {
        let recalled_memories = self
            .memory_service
            .recall()
            .await
            .map_err(to_internal_error)?;

        let selected = recalled_memories.into_iter().take(50).collect::<Vec<_>>();
        to_pretty_json_result(&RecallResponse { memories: selected })
    }

    #[tool(
        name = "remember_short",
        description = "Store session-level context that helps future continuation. Use for recent progress, temporary working context, intermediate conclusions, or resume points."
    )]
    async fn remember_short(
        &self,
        Parameters(request): Parameters<RememberRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        self.remember_with_tier(RetentionTier::Short, request.text)
            .await
    }

    #[tool(
        name = "remember_medium",
        description = "Store information that is useful across sessions but may change over time. Use for evolving preferences, personal events, decisions made, lessons learned."
    )]
    async fn remember_medium(
        &self,
        Parameters(request): Parameters<RememberRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        self.remember_with_tier(RetentionTier::Medium, request.text)
            .await
    }

    #[tool(
        name = "remember_long",
        description = "Store information expected to remain valid over long periods. Use for durable facts, stable constraints, or information with low expected change frequency."
    )]
    async fn remember_long(
        &self,
        Parameters(request): Parameters<RememberRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        self.remember_with_tier(RetentionTier::Long, request.text)
            .await
    }

    #[tool(
        name = "reinforce",
        description = "Strengthen recalled memories that materially influenced your work in the current session. Do not reinforce memories merely because they were present."
    )]
    async fn reinforce(
        &self,
        Parameters(request): Parameters<MemoryIdsRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .memory_service
            .reinforce(&request.memory_ids)
            .await
            .map_err(to_internal_error)?;

        Ok(rejection_result(result.rejection))
    }

    #[tool(
        name = "forget",
        description = "Delete memories by id. Use this when a previously stored memory is wrong or no longer relevant. Prefer targeted deletions; do not mass-delete without a clear reason."
    )]
    async fn forget(
        &self,
        Parameters(request): Parameters<MemoryIdsRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .memory_service
            .forget(&request.memory_ids)
            .await
            .map_err(to_internal_error)?;

        Ok(rejection_result(result.rejection))
    }

    async fn remember_with_tier(
        &self,
        retention_tier: RetentionTier,
        text: String,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .memory_service
            .remember(retention_tier, &text)
            .await
            .map_err(to_internal_error)?;

        Ok(rejection_result(result.rejection))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EngramServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            rmcp::model::Implementation::new("EngramMcp", env!("CARGO_PKG_VERSION")),
        )
    }
}

fn rejection_result(rejection: Option<String>) -> CallToolResult {
    match rejection {
        Some(message) => CallToolResult::success(vec![Content::text(message)]),
        None => CallToolResult::success(vec![]),
    }
}

fn to_pretty_json_result<T: Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    let text = serde_json::to_string_pretty(value).map_err(to_internal_error)?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

fn to_internal_error(error: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(error.to_string(), None)
}

#[cfg(test)]
mod tests {
    use rmcp::{
        ClientHandler, ServiceExt,
        model::{CallToolRequestParams, ClientInfo},
    };

    use super::*;
    use crate::memory_store::{MemoryStore, PersistedMemoryDocument};

    #[derive(Debug, Clone, Default)]
    struct TestClient;

    impl ClientHandler for TestClient {
        fn get_info(&self) -> ClientInfo {
            ClientInfo::default()
        }
    }

    #[tokio::test]
    async fn server_exposes_expected_tools() {
        let temp = tempfile::tempdir().unwrap();
        let store = JsonMemoryStore::new(temp.path().join("memory.json")).unwrap();
        store.ensure_initialized().await.unwrap();

        let server = EngramServer::new(MemoryService::new(store));

        let (server_transport, client_transport) = tokio::io::duplex(8192);

        let server_handle = tokio::spawn(async move {
            let service = server.serve(server_transport).await.unwrap();
            service.waiting().await.unwrap();
        });

        let client = TestClient.serve(client_transport).await.unwrap();
        let tools = client.list_tools(Default::default()).await.unwrap();

        let mut names = tools
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>();
        names.sort();

        assert_eq!(
            names,
            vec![
                "forget",
                "recall",
                "reinforce",
                "remember_long",
                "remember_medium",
                "remember_short"
            ]
        );

        client.cancel().await.unwrap();
        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn remember_short_returns_validation_message_for_invalid_text() {
        let temp = tempfile::tempdir().unwrap();
        let store = JsonMemoryStore::new(temp.path().join("memory.json")).unwrap();
        store.ensure_initialized().await.unwrap();

        let server = EngramServer::new(MemoryService::new(store));

        let (server_transport, client_transport) = tokio::io::duplex(8192);

        let server_handle = tokio::spawn(async move {
            let service = server.serve(server_transport).await.unwrap();
            service.waiting().await.unwrap();
        });

        let client = TestClient.serve(client_transport).await.unwrap();

        let response = client
            .call_tool(
                CallToolRequestParams::new("remember_short").with_arguments(
                    serde_json::json!({ "text": "" })
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();

        let text = response
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .unwrap();
        assert_eq!(
            text.text,
            "Memory text must not be null, empty, or whitespace."
        );

        client.cancel().await.unwrap();
        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn recall_returns_only_id_and_text_shape() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("memory.json");
        let store = JsonMemoryStore::new(file.clone()).unwrap();
        store
            .save(&PersistedMemoryDocument {
                memories: vec![crate::memory_store::PersistedMemory {
                    id: "id-1".to_string(),
                    text: "Known memory".to_string(),
                    retention: 10.0,
                }],
            })
            .await
            .unwrap();

        let server = EngramServer::new(MemoryService::new(store));

        let (server_transport, client_transport) = tokio::io::duplex(8192);

        let server_handle = tokio::spawn(async move {
            let service = server.serve(server_transport).await.unwrap();
            service.waiting().await.unwrap();
        });

        let client = TestClient.serve(client_transport).await.unwrap();
        let response = client
            .call_tool(CallToolRequestParams::new("recall"))
            .await
            .unwrap();

        let text = response
            .content
            .first()
            .and_then(|content| content.raw.as_text())
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&text.text).unwrap();

        assert_eq!(json["memories"][0]["id"], "id-1");
        assert_eq!(json["memories"][0]["text"], "Known memory");
        assert!(json["memories"][0].get("retention").is_none());

        client.cancel().await.unwrap();
        server_handle.await.unwrap();
    }
}
