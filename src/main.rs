use anyhow::{Context, Result};
use engram_mcp::{
    command_line::parse_memory_file_options, memory::MemoryService, memory_store::JsonMemoryStore,
    server::EngramServer,
};
use rmcp::{ServiceExt, transport::stdio};

#[tokio::main]
async fn main() -> Result<()> {
    let current_directory =
        std::env::current_dir().context("Failed to determine current directory.")?;
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let options = parse_memory_file_options(&arguments, &current_directory)
        .map_err(|message| anyhow::anyhow!(message))?;

    let memory_store = JsonMemoryStore::new(options.file_path)?;
    let memory_service = MemoryService::new(memory_store);
    let server = EngramServer::new(memory_service);

    let service = server
        .serve(stdio())
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    service
        .waiting()
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    Ok(())
}
