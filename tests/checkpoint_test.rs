//! Tests for checkpoint_id functionality (MVP)

use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;

use ace_tool::config::{Config, ConfigOptions};
use ace_tool::index::{FileEntry, IndexData, IndexManager};

fn create_test_config() -> Arc<Config> {
    Config::new(
        "https://api.example.com".to_string(),
        "test-token".to_string(),
        ConfigOptions::default(),
    )
    .unwrap()
}

fn create_test_manager(project_root: std::path::PathBuf) -> IndexManager {
    let config = create_test_config();
    IndexManager::new(config, project_root).unwrap()
}

#[test]
fn test_checkpoint_id_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(temp_dir.path().to_path_buf());

    // Create index with checkpoint_id
    let mut index = IndexData {
        version: 3,
        config_hash: manager.config_hash().to_string(),
        entries: HashMap::new(),
        checkpoint_id: Some("test-checkpoint-123".to_string()),
        last_sync_time: Some(1234567890),
    };

    index.entries.insert(
        "test.rs".to_string(),
        FileEntry {
            mtime_secs: 1000,
            mtime_nanos: 0,
            size: 100,
            blob_hashes: vec!["hash1".to_string()],
        },
    );

    // Save index
    manager.save_index(&index).unwrap();

    // Load index and verify checkpoint_id is preserved
    let loaded = manager.load_index();
    assert_eq!(loaded.checkpoint_id, Some("test-checkpoint-123".to_string()));
    assert_eq!(loaded.last_sync_time, Some(1234567890));
    assert_eq!(loaded.entries.len(), 1);
}

#[test]
fn test_checkpoint_id_default_none() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(temp_dir.path().to_path_buf());

    // Load non-existent index
    let index = manager.load_index();

    // Should have None for checkpoint fields
    assert_eq!(index.checkpoint_id, None);
    assert_eq!(index.last_sync_time, None);
    assert!(index.entries.is_empty());
}

#[test]
fn test_checkpoint_id_v2_to_v3_upgrade() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(temp_dir.path().to_path_buf());

    // Create v2 index (without checkpoint fields)
    let mut v2_index = IndexData {
        version: 2,
        config_hash: manager.config_hash().to_string(),
        entries: HashMap::new(),
        checkpoint_id: None,
        last_sync_time: None,
    };

    v2_index.entries.insert(
        "old.rs".to_string(),
        FileEntry {
            mtime_secs: 2000,
            mtime_nanos: 0,
            size: 200,
            blob_hashes: vec!["hash2".to_string()],
        },
    );

    manager.save_index(&v2_index).unwrap();

    // Load should trigger rebuild due to version mismatch
    let loaded = manager.load_index();

    // Should return empty index (rebuild triggered)
    assert!(loaded.entries.is_empty());
    assert_eq!(loaded.checkpoint_id, None);
}

#[test]
fn test_checkpoint_id_clear_on_expired() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(temp_dir.path().to_path_buf());

    // Create index with checkpoint_id
    let mut index = IndexData {
        version: 3,
        config_hash: manager.config_hash().to_string(),
        entries: HashMap::new(),
        checkpoint_id: Some("expired-checkpoint".to_string()),
        last_sync_time: Some(9999999999),
    };

    manager.save_index(&index).unwrap();

    // Simulate clearing checkpoint (as done in 404 handler)
    let mut loaded = manager.load_index();
    assert_eq!(loaded.checkpoint_id, Some("expired-checkpoint".to_string()));

    loaded.checkpoint_id = None;
    loaded.last_sync_time = None;
    manager.save_index(&loaded).unwrap();

    // Verify checkpoint is cleared
    let reloaded = manager.load_index();
    assert_eq!(reloaded.checkpoint_id, None);
    assert_eq!(reloaded.last_sync_time, None);
}

#[test]
fn test_index_data_serialization_with_checkpoint() {
    use serde_json;

    let index = IndexData {
        version: 3,
        config_hash: "test_hash".to_string(),
        entries: HashMap::new(),
        checkpoint_id: Some("cp-123".to_string()),
        last_sync_time: Some(1234567890),
    };

    // Serialize to JSON
    let json = serde_json::to_string(&index).unwrap();

    // Deserialize back
    let deserialized: IndexData = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.version, 3);
    assert_eq!(deserialized.checkpoint_id, Some("cp-123".to_string()));
    assert_eq!(deserialized.last_sync_time, Some(1234567890));
}

#[test]
fn test_index_data_backward_compatibility() {
    use serde_json;

    // Simulate old JSON without checkpoint fields
    let old_json = r#"{
        "version": 2,
        "config_hash": "old_hash",
        "entries": {}
    }"#;

    // Should deserialize with default None values
    let deserialized: IndexData = serde_json::from_str(old_json).unwrap();

    assert_eq!(deserialized.version, 2);
    assert_eq!(deserialized.checkpoint_id, None);
    assert_eq!(deserialized.last_sync_time, None);
}
