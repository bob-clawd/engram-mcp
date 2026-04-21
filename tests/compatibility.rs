use std::path::{Path, PathBuf};

use engram_mcp::{
    memory::MemoryService,
    memory_store::{JsonMemoryStore, MemoryStore},
};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

async fn copy_fixture(name: &str, destination: &Path) {
    let content = tokio::fs::read_to_string(fixture_path(name)).await.unwrap();
    tokio::fs::write(destination, content).await.unwrap();
}

#[tokio::test]
async fn loads_csharp_memory_fixture_without_shape_changes() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("memory.json");
    copy_fixture("csharp-memory.json", &file_path).await;

    let store = JsonMemoryStore::new(file_path).unwrap();
    let document = store.load().await.unwrap();

    assert_eq!(document.memories.len(), 3);
    assert_eq!(document.memories[0].id, "mknown1");
    assert_eq!(document.memories[0].retention, 10.0);
    assert_eq!(document.memories[1].retention, 5.0);
    assert_eq!(document.memories[2].retention, 0.9);
}

#[tokio::test]
async fn recall_prunes_expired_entries_from_csharp_fixture() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("memory.json");
    copy_fixture("csharp-memory.json", &file_path).await;

    let store = JsonMemoryStore::new(file_path.clone()).unwrap();
    let service = MemoryService::new(store.clone());

    let memories = service.recall().await.unwrap();
    let document = store.load().await.unwrap();

    assert_eq!(memories.len(), 2);
    assert_eq!(memories[0].id, "mknown1");
    assert_eq!(memories[1].id, "mknown2");
    assert_eq!(document.memories.len(), 2);
    assert_eq!(document.memories[0].retention, 10.0);
    assert_eq!(document.memories[1].retention, 5.0);
}

#[tokio::test]
async fn saves_whole_number_retention_like_csharp() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("memory.json");
    copy_fixture("csharp-memory.json", &file_path).await;

    let store = JsonMemoryStore::new(file_path.clone()).unwrap();
    let document = store.load().await.unwrap();
    store.save(&document).await.unwrap();

    let content = tokio::fs::read_to_string(file_path).await.unwrap();

    assert!(content.contains("\"retention\": 10"));
    assert!(content.contains("\"retention\": 5"));
    assert!(content.contains("\"retention\": 0.9"));
    assert!(!content.contains("\"retention\": 10.0"));
    assert!(!content.contains("\"retention\": 5.0"));
}

#[tokio::test]
async fn keeps_decimal_retention_from_reinforced_csharp_fixture() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("memory.json");
    copy_fixture("csharp-reinforced-memory.json", &file_path).await;

    let store = JsonMemoryStore::new(file_path.clone()).unwrap();
    let service = MemoryService::new(store.clone());

    let result = service.reinforce(&["anchor".to_string()]).await.unwrap();
    let document = store.load().await.unwrap();

    assert!(result.succeeded());
    assert_eq!(document.memories[0].retention, 9.8);
    assert_eq!(document.memories[1].retention, 0.0);
}
