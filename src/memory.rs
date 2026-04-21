use std::{
    collections::HashSet,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use tokio::sync::Mutex;

use crate::memory_store::{
    MemoryStore, PersistedMemory, PersistedMemoryDocument, get_memory_text_validation_error,
    normalize_retention, require_valid_memory_text,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionTier {
    Short,
    Medium,
    Long,
}

impl RetentionTier {
    pub fn to_value(self) -> f64 {
        match self {
            Self::Short => 5.0,
            Self::Medium => 25.0,
            Self::Long => 100.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RecallMemory {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChangeResult {
    pub rejection: Option<String>,
}

impl MemoryChangeResult {
    pub fn success() -> Self {
        Self { rejection: None }
    }

    pub fn reject(rejection: impl Into<String>) -> Self {
        Self {
            rejection: Some(rejection.into()),
        }
    }

    pub fn succeeded(&self) -> bool {
        self.rejection.is_none()
    }
}

#[derive(Clone)]
pub struct MemoryService<S: MemoryStore> {
    store: S,
    gate: Arc<Mutex<RetentionCycle>>,
}

impl<S: MemoryStore> MemoryService<S> {
    pub fn new(store: S) -> Self {
        Self {
            store,
            gate: Arc::new(Mutex::new(RetentionCycle::new())),
        }
    }

    pub async fn recall(&self) -> Result<Vec<RecallMemory>> {
        let mut cycle = self.gate.lock().await;
        let mut document = self.store.load().await?;

        if prune_deletable_memories(&mut document) {
            self.store.save(&document).await?;
        }

        cycle.reset();

        Ok(recall_memories(document))
    }

    pub async fn remember(
        &self,
        retention_tier: RetentionTier,
        text: &str,
    ) -> Result<MemoryChangeResult> {
        if let Some(error) = get_memory_text_validation_error(text) {
            return Ok(MemoryChangeResult::reject(error));
        }

        let _cycle = self.gate.lock().await;
        let mut document = self.store.load().await?;
        let memory = PersistedMemory {
            id: unique_id(),
            text: require_valid_memory_text(text)?,
            retention: normalize_retention(retention_tier.to_value())?,
        };

        document.memories.push(memory);
        self.store.save(&document).await?;

        Ok(MemoryChangeResult::success())
    }

    pub async fn reinforce(&self, memory_ids: &[String]) -> Result<MemoryChangeResult> {
        if memory_ids.is_empty() {
            return Ok(MemoryChangeResult::reject(
                "At least one memory id is required.",
            ));
        }

        let mut cycle = self.gate.lock().await;
        let mut document = self.store.load().await?;

        if let Some(unknown_memory_id) = get_unknown_memory_id(memory_ids, &document.memories) {
            return Ok(MemoryChangeResult::reject(format!(
                "Unknown memory '{}'.",
                unknown_memory_id
            )));
        }

        let mut changed_retention =
            apply_global_weakening_if_first_time(&mut document, &mut cycle)?;
        changed_retention |= reinforce_memories(&mut document, memory_ids, &mut cycle)?;

        if changed_retention {
            self.store.save(&document).await?;
        }

        Ok(MemoryChangeResult::success())
    }

    pub async fn forget(&self, memory_ids: &[String]) -> Result<MemoryChangeResult> {
        if memory_ids.is_empty() {
            return Ok(MemoryChangeResult::reject(
                "At least one memory id is required.",
            ));
        }

        let _cycle = self.gate.lock().await;
        let mut document = self.store.load().await?;

        if let Some(unknown_memory_id) = get_unknown_memory_id(memory_ids, &document.memories) {
            return Ok(MemoryChangeResult::reject(format!(
                "Unknown memory '{}'.",
                unknown_memory_id
            )));
        }

        let requested_memory_ids = memory_ids.iter().cloned().collect::<HashSet<_>>();
        document
            .memories
            .retain(|memory| !requested_memory_ids.contains(&memory.id));

        self.store.save(&document).await?;
        Ok(MemoryChangeResult::success())
    }
}

fn prune_deletable_memories(document: &mut PersistedMemoryDocument) -> bool {
    let start = document.memories.len();
    document
        .memories
        .retain(|memory| !should_delete(memory.retention));
    start != document.memories.len()
}

fn recall_memories(document: PersistedMemoryDocument) -> Vec<RecallMemory> {
    let mut memories = document.memories;
    memories.sort_by(|left, right| right.retention.total_cmp(&left.retention));

    memories
        .into_iter()
        .map(|memory| RecallMemory {
            id: memory.id,
            text: memory.text,
        })
        .collect()
}

fn get_unknown_memory_id(memory_ids: &[String], memories: &[PersistedMemory]) -> Option<String> {
    let known_memory_ids = memories
        .iter()
        .map(|memory| memory.id.clone())
        .collect::<HashSet<_>>();

    memory_ids.iter().find_map(|memory_id| {
        if memory_id.trim().is_empty() {
            return Some(memory_id.clone());
        }

        if known_memory_ids.contains(memory_id) {
            None
        } else {
            Some(memory_id.clone())
        }
    })
}

fn apply_global_weakening_if_first_time(
    document: &mut PersistedMemoryDocument,
    cycle: &mut RetentionCycle,
) -> Result<bool> {
    if !cycle.can_decay() {
        return Ok(false);
    }

    for memory in &mut document.memories {
        memory.retention = normalize_retention(decay(memory.retention))?;
    }

    Ok(true)
}

fn reinforce_memories(
    document: &mut PersistedMemoryDocument,
    memory_ids: &[String],
    cycle: &mut RetentionCycle,
) -> Result<bool> {
    let requested_memory_ids = memory_ids.iter().cloned().collect::<HashSet<_>>();
    let mut changed_retention = false;

    for memory in &mut document.memories {
        if !requested_memory_ids.contains(&memory.id) || !cycle.can_reinforce(&memory.id) {
            continue;
        }

        memory.retention = normalize_retention(reinforce(memory.retention))?;
        changed_retention = true;
    }

    Ok(changed_retention)
}

fn decay(retention: f64) -> f64 {
    retention - 1.0
}

fn reinforce(retention: f64) -> f64 {
    retention * 1.1
}

fn should_delete(retention: f64) -> bool {
    retention < 1.0
}

fn unique_id() -> String {
    static LAST_TIMESTAMP: std::sync::Mutex<i64> = std::sync::Mutex::new(0);

    let mut last_timestamp = LAST_TIMESTAMP.lock().expect("timestamp lock poisoned");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before UNIX_EPOCH")
        .as_millis() as i64;

    let next_timestamp = timestamp.max(*last_timestamp + 1);
    *last_timestamp = next_timestamp;

    to_base36(next_timestamp)
}

fn to_base36(mut value: i64) -> String {
    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

    if value == 0 {
        return "0".to_string();
    }

    let mut buffer = Vec::new();

    while value > 0 {
        let remainder = (value % 36) as usize;
        buffer.push(ALPHABET[remainder] as char);
        value /= 36;
    }

    buffer.iter().rev().collect()
}

#[derive(Debug)]
struct RetentionCycle {
    decayed: bool,
    reinforced: HashSet<String>,
}

impl RetentionCycle {
    fn new() -> Self {
        Self {
            decayed: false,
            reinforced: HashSet::new(),
        }
    }

    fn reset(&mut self) {
        self.decayed = false;
        self.reinforced.clear();
    }

    fn can_decay(&mut self) -> bool {
        let result = !self.decayed;
        self.decayed = true;
        result
    }

    fn can_reinforce(&mut self, memory_id: &str) -> bool {
        self.reinforced.insert(memory_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use tokio::sync::Mutex;

    use crate::memory_store::MemoryStore;

    #[derive(Clone)]
    struct InMemoryMemoryStore {
        document: std::sync::Arc<Mutex<PersistedMemoryDocument>>,
    }

    impl InMemoryMemoryStore {
        fn new(document: PersistedMemoryDocument) -> Self {
            Self {
                document: std::sync::Arc::new(Mutex::new(document)),
            }
        }

        async fn current_document(&self) -> PersistedMemoryDocument {
            self.document.lock().await.clone()
        }

        async fn replace(&self, document: PersistedMemoryDocument) {
            *self.document.lock().await = document;
        }
    }

    impl MemoryStore for InMemoryMemoryStore {
        async fn ensure_initialized(&self) -> Result<()> {
            Ok(())
        }

        async fn load(&self) -> Result<PersistedMemoryDocument> {
            Ok(self.current_document().await)
        }

        async fn save(&self, document: &PersistedMemoryDocument) -> Result<()> {
            self.replace(document.clone()).await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn recall_prunes_deleteable_memories_without_decay() {
        let store = InMemoryMemoryStore::new(PersistedMemoryDocument {
            memories: vec![
                PersistedMemory {
                    id: "a".to_string(),
                    text: "keep".to_string(),
                    retention: 10.0,
                },
                PersistedMemory {
                    id: "b".to_string(),
                    text: "drop".to_string(),
                    retention: 0.9,
                },
            ],
        });
        let service = MemoryService::new(store.clone());

        let memories = service.recall().await.unwrap();

        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].id, "a");
        assert_eq!(memories[0].text, "keep");

        let document = store.current_document().await;
        assert_eq!(document.memories.len(), 1);
        assert_eq!(document.memories[0].retention, 10.0);
    }

    #[tokio::test]
    async fn remember_creates_memory_with_tier_specific_initial_retention() {
        let store = InMemoryMemoryStore::new(PersistedMemoryDocument::default());
        let service = MemoryService::new(store.clone());

        let result = service
            .remember(RetentionTier::Medium, "Remember this")
            .await
            .unwrap();

        assert!(result.succeeded());
        assert_eq!(result.rejection, None);

        let document = store.current_document().await;
        assert_eq!(document.memories.len(), 1);
        assert_eq!(document.memories[0].text, "Remember this");
        assert_eq!(document.memories[0].retention, 25.0);
    }

    #[tokio::test]
    async fn remember_returns_validation_message_for_invalid_text() {
        let store = InMemoryMemoryStore::new(PersistedMemoryDocument::default());
        let service = MemoryService::new(store.clone());

        let result = service.remember(RetentionTier::Short, "").await.unwrap();

        assert!(!result.succeeded());
        assert_eq!(
            result.rejection,
            Some("Memory text must not be null, empty, or whitespace.".to_string())
        );

        let document = store.current_document().await;
        assert!(document.memories.is_empty());
    }

    #[tokio::test]
    async fn reinforce_rejects_unknown_ids_atomically_without_consuming_cycle_weakening() {
        let store = InMemoryMemoryStore::new(PersistedMemoryDocument {
            memories: vec![PersistedMemory {
                id: "known".to_string(),
                text: "Known memory".to_string(),
                retention: 10.0,
            }],
        });
        let service = MemoryService::new(store.clone());

        let rejected = service
            .reinforce(&["known".to_string(), "missing".to_string()])
            .await
            .unwrap();
        let accepted = service.reinforce(&["known".to_string()]).await.unwrap();

        assert!(!rejected.succeeded());
        assert_eq!(
            rejected.rejection,
            Some("Unknown memory 'missing'.".to_string())
        );
        assert!(accepted.succeeded());

        let document = store.current_document().await;
        assert_eq!(document.memories[0].retention, 9.9);
    }

    #[tokio::test]
    async fn forget_returns_validation_message_for_unknown_memory() {
        let store = InMemoryMemoryStore::new(PersistedMemoryDocument {
            memories: vec![PersistedMemory {
                id: "id-1".to_string(),
                text: "First memory".to_string(),
                retention: 10.0,
            }],
        });
        let service = MemoryService::new(store);

        let response = service.forget(&["id-2".to_string()]).await.unwrap();

        assert_eq!(
            response.rejection,
            Some("Unknown memory 'id-2'.".to_string())
        );
    }

    #[test]
    fn unique_id_returns_unique_ids_for_consecutive_calls() {
        let mut ids = HashSet::new();

        for _ in 0..100 {
            ids.insert(unique_id());
        }

        assert_eq!(ids.len(), 100);
    }

    #[tokio::test]
    async fn unique_id_returns_unique_ids_for_parallel_calls() {
        let tasks = (0..100)
            .map(|_| tokio::spawn(async { unique_id() }))
            .collect::<Vec<_>>();

        let mut ids = HashSet::new();
        for task in tasks {
            ids.insert(task.await.unwrap());
        }

        assert_eq!(ids.len(), 100);
    }

    #[test]
    fn validate_id_rejects_whitespace() {
        let error = crate::memory_store::validate_id("   ")
            .unwrap_err()
            .to_string();
        assert!(error.contains("Memory id must not be null, empty, or whitespace."));
    }
}
