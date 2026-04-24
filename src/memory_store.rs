use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedMemory {
    pub id: String,
    pub text: String,
    #[serde(serialize_with = "serialize_retention")]
    pub retention: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PersistedMemoryDocument {
    pub memories: Vec<PersistedMemory>,
}

#[allow(async_fn_in_trait)]
pub trait MemoryStore: Send + Sync + Clone + 'static {
    async fn ensure_initialized(&self) -> Result<()>;
    async fn load(&self) -> Result<PersistedMemoryDocument>;
    async fn save(&self, document: &PersistedMemoryDocument) -> Result<()>;
}

#[derive(Clone)]
pub struct JsonMemoryStore {
    state: Arc<JsonMemoryStoreState>,
}

struct JsonMemoryStoreState {
    file_path: PathBuf,
    gate: Mutex<()>,
}

impl JsonMemoryStore {
    pub fn new(configured_path: PathBuf) -> Result<Self> {
        if configured_path
            .to_str()
            .is_none_or(|value| value.trim().is_empty())
        {
            bail!("The configured memory file path must not be empty or whitespace.");
        }

        let resolved = resolve_path(configured_path)?;

        Ok(Self {
            state: Arc::new(JsonMemoryStoreState {
                file_path: resolved,
                gate: Mutex::new(()),
            }),
        })
    }

    pub fn file_path(&self) -> &Path {
        &self.state.file_path
    }

    async fn ensure_initialized_inner(&self) -> Result<()> {
        let directory_path = self
            .state
            .file_path
            .parent()
            .context("Memory file path has no parent directory.")?;

        tokio::fs::create_dir_all(directory_path)
            .await
            .with_context(|| {
                format!(
                    "Memory file path '{}' could not be initialized.",
                    self.state.file_path.display()
                )
            })?;

        if !tokio::fs::try_exists(&self.state.file_path).await? {
            let empty_document = PersistedMemoryDocument::default();
            self.save_inner(&empty_document).await?;
        }

        Ok(())
    }

    async fn load_inner(&self) -> Result<PersistedMemoryDocument> {
        self.ensure_initialized_inner().await?;

        let json = tokio::fs::read_to_string(&self.state.file_path)
            .await
            .with_context(|| {
                format!(
                    "Memory file '{}' could not be read.",
                    self.state.file_path.display()
                )
            })?;

        let mut document: PersistedMemoryDocument =
            serde_json::from_str(&json).with_context(|| {
                format!(
                    "Memory file '{}' contains malformed JSON.",
                    self.state.file_path.display()
                )
            })?;

        validate_document(&mut document)?;
        Ok(document)
    }

    async fn save_inner(&self, document: &PersistedMemoryDocument) -> Result<()> {
        let mut normalized_document = document.clone();
        validate_document(&mut normalized_document)?;

        let directory_path = self
            .state
            .file_path
            .parent()
            .context("Memory file path has no parent directory.")?;

        tokio::fs::create_dir_all(directory_path)
            .await
            .with_context(|| {
                format!(
                    "Memory file path '{}' could not be initialized.",
                    self.state.file_path.display()
                )
            })?;

        let json = serde_json::to_string_pretty(&normalized_document)?;

        // Avoid partially-written JSON on crash: write temp file next to destination, then rename.
        let tmp_path = self.state.file_path.with_extension("json.tmp");
        tokio::fs::write(&tmp_path, json).await.with_context(|| {
            format!("Memory file '{}' could not be written.", tmp_path.display())
        })?;

        // Windows rename fails if destination exists.
        if tokio::fs::try_exists(&self.state.file_path).await? {
            let _ = tokio::fs::remove_file(&self.state.file_path).await;
        }

        tokio::fs::rename(&tmp_path, &self.state.file_path)
            .await
            .with_context(|| {
                format!(
                    "Memory file path '{}' could not be written.",
                    self.state.file_path.display()
                )
            })?;

        Ok(())
    }
}

impl MemoryStore for JsonMemoryStore {
    async fn ensure_initialized(&self) -> Result<()> {
        let _guard = self.state.gate.lock().await;
        self.ensure_initialized_inner().await
    }

    async fn load(&self) -> Result<PersistedMemoryDocument> {
        let _guard = self.state.gate.lock().await;
        self.load_inner().await
    }

    async fn save(&self, document: &PersistedMemoryDocument) -> Result<()> {
        let _guard = self.state.gate.lock().await;
        self.save_inner(document).await
    }
}

fn resolve_path(configured_path: PathBuf) -> Result<PathBuf> {
    if configured_path.is_absolute() {
        return Ok(configured_path);
    }

    let current_directory =
        std::env::current_dir().context("Failed to determine current directory.")?;
    Ok(current_directory.join(configured_path))
}

fn validate_document(document: &mut PersistedMemoryDocument) -> Result<()> {
    let mut ids = HashSet::new();

    for memory in &mut document.memories {
        memory.id = validate_id(&memory.id)?;
        memory.text = require_valid_memory_text(&memory.text)?;
        memory.retention = normalize_retention(memory.retention)?;

        if !ids.insert(memory.id.clone()) {
            bail!(
                "Memory file has invalid structure. Duplicate memory id '{}'.",
                memory.id
            );
        }
    }

    Ok(())
}

pub fn validate_id(id: &str) -> Result<String> {
    if id.trim().is_empty() {
        bail!("Memory id must not be null, empty, or whitespace.");
    }

    Ok(id.trim().to_string())
}

pub fn normalize_retention(retention: f64) -> Result<f64> {
    if !retention.is_finite() || retention < 0.0 {
        bail!("Retention must be a finite non-negative number.");
    }

    let rounded = (retention.min(150.0) * 10.0).round() / 10.0;
    Ok(rounded)
}

pub fn get_memory_text_validation_error(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return Some("Memory text must not be null, empty, or whitespace.".to_string());
    }

    if text.contains('\r') || text.contains('\n') {
        return Some(
            "Memory text must be a single line without carriage returns or line feeds.".to_string(),
        );
    }

    if text.chars().count() > 1000 {
        return Some("Memory text must be 1000 characters or fewer.".to_string());
    }

    None
}

pub fn require_valid_memory_text(text: &str) -> Result<String> {
    if let Some(error) = get_memory_text_validation_error(text) {
        bail!(error);
    }

    Ok(text.to_string())
}

fn serialize_retention<S>(retention: &f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    if retention.fract() == 0.0 {
        return serializer.serialize_i64(*retention as i64);
    }

    serializer.serialize_f64(*retention)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn ensure_initialized_creates_empty_memory_document() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("memory.json");
        let store = JsonMemoryStore::new(file.clone()).unwrap();

        store.ensure_initialized().await.unwrap();

        assert!(file.exists());
        let content = tokio::fs::read_to_string(&file).await.unwrap();
        assert!(content.contains("\"memories\": []"));
    }

    #[tokio::test]
    async fn save_persists_memory_entries_to_json_file() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("memory.json");
        let store = JsonMemoryStore::new(file.clone()).unwrap();

        let document = PersistedMemoryDocument {
            memories: vec![PersistedMemory {
                id: "260329142501".to_string(),
                text: "Durable fact".to_string(),
                retention: 10.0,
            }],
        };

        store.save(&document).await.unwrap();

        let content = tokio::fs::read_to_string(&file).await.unwrap();
        assert!(content.contains("Durable fact"));
        assert!(content.contains("\"id\": \"260329142501\""));
        assert!(content.contains("\"retention\": 10\n"));
    }

    #[tokio::test]
    async fn load_reads_existing_memories_from_json_file() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("memory.json");
        tokio::fs::write(
            &file,
            r#"{
  "memories": [
    {
      "id": "260329142501",
      "text": "Remember project detail",
      "retention": 10
    }
  ]
}"#,
        )
        .await
        .unwrap();

        let store = JsonMemoryStore::new(file).unwrap();
        let document = store.load().await.unwrap();

        assert_eq!(document.memories.len(), 1);
        assert_eq!(document.memories[0].id, "260329142501");
        assert_eq!(document.memories[0].text, "Remember project detail");
        assert_eq!(document.memories[0].retention, 10.0);
    }

    #[test]
    fn serialize_retention_keeps_fraction_only_when_needed() {
        let whole_number = serde_json::to_string(&PersistedMemory {
            id: "id-1".to_string(),
            text: "Whole".to_string(),
            retention: 5.0,
        })
        .unwrap();
        let fractional_number = serde_json::to_string(&PersistedMemory {
            id: "id-2".to_string(),
            text: "Fraction".to_string(),
            retention: 9.9,
        })
        .unwrap();

        assert!(whole_number.contains("\"retention\":5"));
        assert!(fractional_number.contains("\"retention\":9.9"));
    }
}
