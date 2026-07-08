//! Integration tests for bidirectional UUID reconciliation (INDEX-010)
//!
//! The ephemeral index assigns temporary v4 UUIDs during discovery. When the
//! scanned path overlaps the persistent index, a reconciliation pass replaces
//! them with the persistent UUIDs so tags, selections, and metadata attached
//! to persistent entries stay visible in ephemeral views.

mod helpers;

use helpers::*;
use sd_core::{
	location::IndexMode,
	ops::indexing::{
		reconciliation::extract_persistent_uuids_for_path, state::EntryKind, EntryMetadata,
	},
};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use uuid::Uuid;

fn ephemeral_file_metadata(path: &Path) -> EntryMetadata {
	EntryMetadata {
		path: path.to_path_buf(),
		kind: EntryKind::File,
		size: 4,
		modified: Some(SystemTime::UNIX_EPOCH),
		accessed: None,
		created: Some(SystemTime::UNIX_EPOCH),
		inode: None,
		permissions: None,
		is_hidden: false,
	}
}

#[tokio::test]
async fn test_ephemeral_reconciliation_adopts_persistent_uuids() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("uuid_reconciliation")
		.disable_watcher()
		.build()
		.await?;

	let test_location = harness.create_test_location("recon_root").await?;
	test_location.write_file("root_file.txt", "root").await?;
	test_location
		.write_file("sub/nested_file.txt", "nested")
		.await?;

	let location = test_location.index("Recon", IndexMode::Shallow).await?;
	let entries = location.get_all_entries().await?;

	// Simulate an ephemeral browse of the same directory assigning a
	// temporary v4 UUID.
	let cache = harness.core.context.ephemeral_cache().clone();
	let root: PathBuf = test_location.path().to_path_buf();
	let file_path = root.join("root_file.txt");
	let ephemeral_uuid = Uuid::new_v4();
	{
		let index = cache.get_global_index();
		let mut index = index.write().await;
		index.add_entry(
			file_path.clone(),
			ephemeral_uuid,
			ephemeral_file_metadata(&file_path),
		)?;
	}

	// Direction 1: the scanned path is itself a persistent directory.
	let conn = harness.library.db().conn();
	let uuids = extract_persistent_uuids_for_path(conn, &root).await?;
	let persistent_uuid = *uuids
		.get(&file_path)
		.expect("extraction must include the persistently indexed file");

	// The extracted UUID matches the persistent entry's UUID.
	let db_uuid = entries
		.iter()
		.find(|e| e.name.starts_with("root_file"))
		.and_then(|e| e.uuid)
		.expect("persistent entry has a uuid");
	assert_eq!(persistent_uuid, db_uuid);

	// Reconcile: the ephemeral index adopts the persistent UUID.
	let changes = cache.reconcile_persistent_uuids(&uuids).await;
	let change = changes
		.iter()
		.find(|c| c.path == file_path)
		.expect("reconciliation must report the replaced uuid");
	assert_eq!(change.previous, Some(ephemeral_uuid));
	assert_eq!(change.uuid, persistent_uuid);

	{
		let index = cache.get_global_index();
		let index = index.read().await;
		assert_eq!(index.get_entry_uuid(&file_path), Some(persistent_uuid));
	}

	// Re-reconciling with the same map is a no-op.
	let changes = cache.reconcile_persistent_uuids(&uuids).await;
	assert!(
		changes.iter().all(|c| c.path != file_path),
		"idempotent reconciliation must not report unchanged uuids"
	);

	harness.shutdown().await
}

#[tokio::test]
async fn test_parent_scan_extracts_nested_location_uuids() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("uuid_recon_parent_dir")
		.disable_watcher()
		.build()
		.await?;

	let test_location = harness.create_test_location("recon_child").await?;
	test_location.write_file("root_file.txt", "root").await?;

	test_location
		.index("ReconChild", IndexMode::Shallow)
		.await?;

	let root: PathBuf = test_location.path().to_path_buf();
	let scan_root = root
		.parent()
		.expect("test location has a parent directory")
		.to_path_buf();

	// Direction 2: the scanned path is an ancestor of the persistent
	// location root and is itself absent from directory_paths.
	let conn = harness.library.db().conn();
	let uuids = extract_persistent_uuids_for_path(conn, &scan_root).await?;

	assert!(
		uuids.contains_key(&root),
		"location root must be extracted when scanning its parent"
	);
	assert!(
		uuids.contains_key(&root.join("root_file.txt")),
		"location contents must be extracted when scanning an ancestor"
	);

	harness.shutdown().await
}
