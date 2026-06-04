use std::{
    cmp::Reverse,
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyEntry {
    pub id: String,
    pub label: Option<String>,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatedApiKey {
    #[serde(flatten)]
    pub entry: ApiKeyEntry,
    pub key: String,
}

#[derive(Debug, Clone, Default)]
pub struct ApiKeyStore {
    inner: Arc<RwLock<HashMap<String, StoredApiKey>>>,
}

#[derive(Debug, Clone)]
struct StoredApiKey {
    entry: ApiKeyEntry,
    key: String,
}

impl ApiKeyStore {
    pub async fn list(&self) -> Vec<ApiKeyEntry> {
        let keys = self.inner.read().await;
        let mut entries = keys
            .values()
            .map(|stored| stored.entry.clone())
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| Reverse(entry.created_at));
        entries
    }

    pub async fn create(&self, label: Option<String>) -> CreatedApiKey {
        let id = Uuid::new_v4().to_string();
        let key = format!("sk-{}", Uuid::new_v4().simple());
        let entry = ApiKeyEntry {
            id: id.clone(),
            label: clean_label(label),
            created_at: now(),
            last_used_at: None,
        };
        self.inner.write().await.insert(
            id,
            StoredApiKey {
                entry: entry.clone(),
                key: key.clone(),
            },
        );
        CreatedApiKey { entry, key }
    }

    pub async fn get(&self, id: &str) -> Option<ApiKeyEntry> {
        self.inner
            .read()
            .await
            .get(id)
            .map(|stored| stored.entry.clone())
    }

    pub async fn update(&self, id: &str, label: Option<String>) -> Option<ApiKeyEntry> {
        let mut keys = self.inner.write().await;
        let stored = keys.get_mut(id)?;
        stored.entry.label = clean_label(label);
        Some(stored.entry.clone())
    }

    pub async fn delete(&self, id: &str) -> bool {
        self.inner.write().await.remove(id).is_some()
    }

    pub async fn authenticate(&self, presented: &str) -> bool {
        let mut keys = self.inner.write().await;
        for stored in keys.values_mut() {
            if stored.key == presented {
                stored.entry.last_used_at = Some(now());
                return true;
            }
        }
        false
    }
}

fn clean_label(label: Option<String>) -> Option<String> {
    label.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::ApiKeyStore;

    #[tokio::test]
    async fn generated_keys_use_sk_prefix() {
        let created = ApiKeyStore::default().create(None).await;
        assert!(created.key.starts_with("sk-"));
    }
}
