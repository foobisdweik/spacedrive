//! Indexing Integration Test
//!
//! Tests the production indexer functionality including:
//! - Location creation and indexing
//! - Smart filtering of system files
//! - Inode tracking for incremental indexing
//! - Change detection (new, modified, moved, deleted files)
//! - Re-indexing and incremental updates

mod helpers;

use anyhow::{anyhow, Result};
use helpers::{wait_for_indexing, IndexingHarnessBuilder};
use sd_core::{
	domain::{addressing::SdPath, Location, ScanState},
	infra::{
		action::LibraryAction,
		api::SessionContext,
		db::entities,
		event::{Event, EventBus, EventSubscriber, SubscriptionFilter},
		query::LibraryQuery,
	},
	location::{IndexMode, LocationManager},
	ops::locations::{
		enable_indexing::{EnableIndexingAction, EnableIndexingInput},
		list::{LocationsListQuery, LocationsListQueryInput},
	},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use tokio::time::{timeout, Duration, Instant};
use uuid::Uuid;

fn subscribe_to_location_events(event_bus: &EventBus) -> EventSubscriber {
	event_bus.subscribe_filtered(vec![SubscriptionFilter::Global {
		resource_type: "location".to_string(),
	}])
}

async fn collect_location_events_until<F>(
	subscriber: &mut EventSubscriber,
	location_id: Uuid,
	timeout_duration: Duration,
	done: F,
) -> Result<Vec<Location>>
where
	F: Fn(&Location) -> bool,
{
	let mut locations = Vec::new();
	let deadline = Instant::now() + timeout_duration;

	loop {
		let remaining = deadline.saturating_duration_since(Instant::now());
		let event = timeout(remaining, subscriber.recv())
			.await
			.map_err(|_| anyhow!("timed out waiting for location {location_id} events"))?
			.map_err(|error| anyhow!("location event subscription failed: {error}"))?;

		let Event::ResourceChangedBatch { resources, .. } = event else {
			continue;
		};

		let matching_locations = resources
			.as_array()
			.into_iter()
			.flatten()
			.filter_map(|resource| serde_json::from_value::<Location>(resource.clone()).ok())
			.filter(|location| location.id == location_id)
			.collect::<Vec<_>>();
		let reached_done = matching_locations.iter().any(&done);
		locations.extend(matching_locations);

		if reached_done {
			return Ok(locations);
		}
	}
}

#[tokio::test]
async fn test_enable_indexing_emits_mode_before_single_scanning_transition() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("enable_indexing_events")
		.build()
		.await?;
	let test_location = harness
		.create_test_location("enable_indexing_location")
		.await?;
	test_location.write_file("test.txt", "indexed").await?;

	let location_manager = LocationManager::new((*harness.core.events).clone());
	let location_path = SdPath::new(
		sd_core::device::get_current_device_slug(),
		test_location.path().to_path_buf(),
	);
	let (location_id, _) = location_manager
		.add_location(
			harness.library.clone(),
			location_path,
			Some("Enable Indexing".to_string()),
			harness.device_db_id,
			IndexMode::None,
			None,
			None,
			&harness.core.context.volume_manager,
		)
		.await?;
	let location_record = entities::location::Entity::find()
		.filter(entities::location::Column::Uuid.eq(location_id))
		.one(harness.library.db().conn())
		.await?
		.expect("location should exist before enabling indexing");

	let mut event_subscriber = subscribe_to_location_events(harness.library.event_bus());
	EnableIndexingAction::new(EnableIndexingInput {
		id: location_id,
		index_mode: "deep".to_string(),
	})
	.execute(harness.library.clone(), harness.core.context.clone())
	.await?;
	wait_for_indexing(
		&harness.library,
		location_record.id,
		Duration::from_secs(30),
	)
	.await?;

	let location_events = collect_location_events_until(
		&mut event_subscriber,
		location_id,
		Duration::from_secs(5),
		|location| location.scan_state == ScanState::Completed,
	)
	.await?;
	assert!(
		location_events
			.iter()
			.all(|location| location.library_id == harness.library.id()),
		"location events must preserve the owning library ID"
	);

	let mode_event_position = location_events
		.iter()
		.position(|location| {
			location.index_mode == sd_core::domain::IndexMode::Deep
				&& location.scan_state == ScanState::Idle
		})
		.expect("index-mode update should be emitted before dispatch");
	let scanning_positions: Vec<_> = location_events
		.iter()
		.enumerate()
		.filter_map(|(position, location)| {
			matches!(location.scan_state, ScanState::Scanning { .. }).then_some(position)
		})
		.collect();
	assert_eq!(
		scanning_positions.len(),
		1,
		"the indexer job should own the only scanning transition"
	);
	assert!(
		mode_event_position < scanning_positions[0],
		"the durable index-mode event must precede scanning"
	);
	let completed_position = location_events
		.iter()
		.position(|location| location.scan_state == ScanState::Completed)
		.expect("completed indexing should emit the final location state");
	assert!(scanning_positions[0] < completed_position);

	harness.shutdown().await?;

	let failure_harness = IndexingHarnessBuilder::new("enable_indexing_dispatch_failure")
		.build()
		.await?;
	let failure_location = failure_harness
		.create_test_location("dispatch_failure_location")
		.await?;
	let failure_manager = LocationManager::new((*failure_harness.core.events).clone());
	let failure_path = SdPath::new(
		sd_core::device::get_current_device_slug(),
		failure_location.path().to_path_buf(),
	);
	let (failure_location_id, _) = failure_manager
		.add_location(
			failure_harness.library.clone(),
			failure_path,
			Some("Dispatch Failure".to_string()),
			failure_harness.device_db_id,
			IndexMode::None,
			None,
			None,
			&failure_harness.core.context.volume_manager,
		)
		.await?;
	tokio::fs::remove_dir_all(failure_location.path()).await?;
	let mut failure_events = subscribe_to_location_events(failure_harness.library.event_bus());

	failure_harness
		.library
		.jobs()
		.database()
		.conn()
		.clone()
		.close()
		.await?;
	let dispatch_result = EnableIndexingAction::new(EnableIndexingInput {
		id: failure_location_id,
		index_mode: "quick".to_string(),
	})
	.execute(
		failure_harness.library.clone(),
		failure_harness.core.context.clone(),
	)
	.await;
	assert!(
		dispatch_result.is_err(),
		"closed job database should make dispatch fail"
	);

	let persisted_location = entities::location::Entity::find()
		.filter(entities::location::Column::Uuid.eq(failure_location_id))
		.one(failure_harness.library.db().conn())
		.await?
		.expect("location should remain after dispatch failure");
	assert_eq!(persisted_location.index_mode, "content");
	assert_eq!(
		persisted_location.scan_state, "pending",
		"dispatch failure must not persist a scanning transition"
	);

	let failure_location_events = collect_location_events_until(
		&mut failure_events,
		failure_location_id,
		Duration::from_secs(5),
		|location| {
			location.index_mode == sd_core::domain::IndexMode::Content
				&& location.scan_state == ScanState::Idle
		},
	)
	.await?;
	assert!(
		failure_location_events.iter().any(|location| {
			location.index_mode == sd_core::domain::IndexMode::Content
				&& location.scan_state == ScanState::Idle
				&& !location.is_available
		}),
		"dispatch failure must emit the durable mode and unavailable path state"
	);

	failure_harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn test_basic_indexing() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("basic_indexing")
		.build()
		.await?;

	// Create a test location with files
	let location = harness.create_test_location("test_location").await?;
	location.write_file("test1.txt", "Hello World").await?;
	location.write_file("test2.rs", "fn main() {}").await?;
	location.create_dir("subdir").await?;
	location.write_file("subdir/test3.md", "# Test").await?;

	// Create files that should be filtered
	location.create_filtered_files().await?;

	let mut event_subscriber = subscribe_to_location_events(harness.library.event_bus());

	// Index the location
	let handle = location.index("Test Location", IndexMode::Deep).await?;

	// Verify counts
	let file_count = handle.count_files().await?;
	let dir_count = handle.count_directories().await?;

	assert_eq!(file_count, 3, "Should index 3 files (excluding filtered)");
	assert!(dir_count >= 1, "Should index at least 1 directory (subdir)");

	let location_record = entities::location::Entity::find()
		.filter(entities::location::Column::Uuid.eq(handle.uuid))
		.one(harness.library.db().conn())
		.await?
		.expect("indexed location should still exist");
	assert_eq!(location_record.scan_state, "completed");
	assert_eq!(location_record.total_file_count, 3);

	let emitted_location = collect_location_events_until(
		&mut event_subscriber,
		handle.uuid,
		Duration::from_secs(5),
		|location| location.scan_state == ScanState::Completed,
	)
	.await?
	.into_iter()
	.find(|location| location.scan_state == ScanState::Completed)
	.expect("completed indexing should emit the persisted location");

	let mut session = SessionContext::device_session(
		harness.device_id,
		sd_core::device::get_current_device_slug(),
	);
	session.current_library_id = Some(harness.library.id());
	let listed_locations = LocationsListQuery::from_input(LocationsListQueryInput)?
		.execute(harness.core.context.clone(), session)
		.await?;
	let listed_location = listed_locations
		.locations
		.into_iter()
		.find(|location| location.id == handle.uuid)
		.expect("indexed location should be returned by locations.list");

	assert_eq!(emitted_location.file_count, listed_location.file_count);
	assert_eq!(emitted_location.total_size, listed_location.total_size);
	assert_eq!(emitted_location.scan_state, listed_location.scan_state);
	assert_eq!(emitted_location.library_id, harness.library.id());
	assert_eq!(emitted_location.library_id, listed_location.library_id);

	// Verify smart filtering worked
	handle.verify_no_filtered_entries().await?;

	// Verify inode tracking
	handle.verify_inode_tracking().await?;

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn test_change_detection_new_files() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("change_detection_new")
		.build()
		.await?;

	// Create initial location with files
	let location = harness.create_test_location("test_location").await?;
	location.write_file("file1.txt", "Initial content").await?;
	location.write_file("file2.txt", "More content").await?;

	// Initial indexing
	let handle = location.index("Test Location", IndexMode::Deep).await?;

	let initial_files = handle.count_files().await?;
	assert_eq!(initial_files, 2, "Should have 2 initial files");

	// Add new files
	handle.write_file("file3.txt", "New file").await?;
	handle.write_file("subdir/file4.txt", "Nested file").await?;

	// Re-index to detect new files
	handle.reindex().await?;

	// Verify new files were detected and indexed
	let final_files = handle.count_files().await?;
	assert_eq!(
		final_files, 4,
		"Should detect and index 2 new files (total 4)"
	);

	// Capture UUIDs after first reindex
	let entries_after_first = handle.get_all_entries().await?;
	let file3_uuid = entries_after_first
		.iter()
		.find(|e| e.name == "file3")
		.expect("file3 should exist")
		.uuid;

	// Reindex again without any changes - UUIDs should be preserved
	handle.reindex().await?;

	let entries_after_second = handle.get_all_entries().await?;
	let file3_after = entries_after_second
		.iter()
		.find(|e| e.name == "file3")
		.expect("file3 should still exist");

	assert_eq!(
		file3_uuid, file3_after.uuid,
		"Entry UUID should be preserved across reindexing with inode tracking"
	);

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn test_change_detection_modified_files() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("change_detection_modified")
		.build()
		.await?;

	// Create initial location
	let location = harness.create_test_location("test_location").await?;
	location
		.write_file("mutable.txt", "Original content")
		.await?;

	// Initial indexing
	let handle = location.index("Test Location", IndexMode::Deep).await?;

	// Get initial entry state
	let entries_before = handle.get_all_entries().await?;
	let file_before = entries_before
		.iter()
		.find(|e| e.name == "mutable")
		.expect("File should exist");
	let size_before = file_before.size;

	// Modify the file (change content and size)
	handle
		.modify_file("mutable.txt", "Modified content with more data")
		.await?;

	// Re-index to detect modification
	handle.reindex().await?;

	// Verify file was detected as modified
	let entries_after = handle.get_all_entries().await?;
	let file_after = entries_after
		.iter()
		.find(|e| e.name == "mutable")
		.expect("File should still exist");
	let size_after = file_after.size;

	assert_ne!(
		size_before, size_after,
		"File size should have changed after modification"
	);
	assert!(size_after > size_before, "Modified file should be larger");

	// Verify same entry ID and UUID (updated in place, not recreated)
	assert_eq!(
		file_before.id, file_after.id,
		"Entry ID should be preserved (updated in place, not recreated)"
	);
	assert_eq!(
		file_before.uuid, file_after.uuid,
		"Entry UUID should be preserved with inode tracking"
	);

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn test_change_detection_deleted_files() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("change_detection_deleted")
		.build()
		.await?;

	// Create initial location with files
	let location = harness.create_test_location("test_location").await?;
	location.write_file("file1.txt", "Keep me").await?;
	location.write_file("file2.txt", "Delete me").await?;
	location.write_file("file3.txt", "Also keep me").await?;

	// Initial indexing
	let handle = location.index("Test Location", IndexMode::Deep).await?;

	let initial_files = handle.count_files().await?;
	assert_eq!(initial_files, 3, "Should have 3 initial files");

	// Delete one file
	handle.delete_file("file2.txt").await?;

	// Re-index to detect deletion
	handle.reindex().await?;

	// Verify file was detected as deleted
	let final_files = handle.count_files().await?;
	assert_eq!(final_files, 2, "Should have 2 files after deletion");

	// Verify the deleted file is no longer in the database
	let entries = handle.get_all_entries().await?;
	let deleted_file_exists = entries.iter().any(|e| e.name == "file2");
	assert!(
		!deleted_file_exists,
		"Deleted file should not be in database"
	);

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn test_change_detection_moved_files() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("change_detection_moved")
		.build()
		.await?;

	// Create initial location with files
	let location = harness.create_test_location("test_location").await?;
	location.write_file("original.txt", "Move me").await?;
	location.create_dir("subdir").await?;

	// Initial indexing
	let handle = location.index("Test Location", IndexMode::Deep).await?;

	// Get initial entry state (to verify inode and UUID preservation)
	let entries_before = handle.get_all_entries().await?;
	let file_before = entries_before
		.iter()
		.find(|e| e.name == "original")
		.expect("File should exist");
	let inode_before = file_before.inode;
	let uuid_before = file_before.uuid;

	let initial_files = handle.count_files().await?;
	assert_eq!(initial_files, 1, "Should have 1 file initially");

	// Move the file
	handle.move_file("original.txt", "subdir/moved.txt").await?;

	// Re-index to detect move
	handle.reindex().await?;

	// Verify file still exists with new name
	let entries_after = handle.get_all_entries().await?;

	// Debug: print all entry names
	println!("Entries after re-index:");
	for entry in &entries_after {
		println!(
			"  - {} (kind: {}, inode: {:?})",
			entry.name, entry.kind, entry.inode
		);
	}

	let moved_file = entries_after
		.iter()
		.find(|e| e.name == "moved")
		.expect("Moved file should exist with new name");

	// Verify inode and UUID are preserved (proves it's the same file, not delete+create)
	assert_eq!(
		inode_before, moved_file.inode,
		"Inode should be preserved after move"
	);
	assert_eq!(
		uuid_before, moved_file.uuid,
		"Entry UUID should be preserved after move with inode tracking"
	);

	// Verify old file doesn't exist
	let old_file_exists = entries_after.iter().any(|e| e.name == "original");
	assert!(!old_file_exists, "Old filename should not exist");

	// Verify total file count is still 1 (move, not copy)
	let final_files = handle.count_files().await?;
	assert_eq!(final_files, 1, "Should still have 1 file after move");

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn test_change_detection_batch_changes() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("change_detection_batch")
		.build()
		.await?;

	// Create initial location
	let location = harness.create_test_location("test_location").await?;
	location.write_file("keep1.txt", "Keep").await?;
	location.write_file("modify.txt", "Original").await?;
	location.write_file("delete.txt", "Remove me").await?;
	location.write_file("move.txt", "Move me").await?;

	// Initial indexing
	let handle = location.index("Test Location", IndexMode::Deep).await?;

	let initial_files = handle.count_files().await?;
	assert_eq!(initial_files, 4, "Should have 4 initial files");

	// Capture UUIDs of files we'll modify/move to verify they're preserved
	let entries_before = handle.get_all_entries().await?;
	let modify_uuid_before = entries_before
		.iter()
		.find(|e| e.name == "modify")
		.expect("modify.txt should exist")
		.uuid;
	let move_uuid_before = entries_before
		.iter()
		.find(|e| e.name == "move")
		.expect("move.txt should exist")
		.uuid;

	// Make multiple changes at once
	handle.write_file("new.txt", "Brand new").await?; // New
	handle.modify_file("modify.txt", "Modified content").await?; // Modified
	handle.delete_file("delete.txt").await?; // Deleted
	handle.move_file("move.txt", "moved.txt").await?; // Moved

	// Re-index to detect all changes
	handle.reindex().await?;

	// Verify final state
	let final_files = handle.count_files().await?;
	assert_eq!(
		final_files, 4,
		"Should have 4 files: keep1, modify, new, moved"
	);

	let entries = handle.get_all_entries().await?;
	let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();

	assert!(names.contains(&"keep1"), "Original file should remain");
	assert!(names.contains(&"modify"), "Modified file should remain");
	assert!(names.contains(&"new"), "New file should be added");
	assert!(names.contains(&"moved"), "Moved file should have new name");
	assert!(!names.contains(&"delete"), "Deleted file should be gone");
	assert!(!names.contains(&"move"), "Old move name should be gone");

	// Verify UUIDs are preserved for modified and moved files (inode tracking)
	let modify_after = entries
		.iter()
		.find(|e| e.name == "modify")
		.expect("modify should exist");
	let moved_after = entries
		.iter()
		.find(|e| e.name == "moved")
		.expect("moved should exist");

	assert_eq!(
		modify_uuid_before, modify_after.uuid,
		"Modified file should keep same UUID with inode tracking"
	);
	assert_eq!(
		move_uuid_before, moved_after.uuid,
		"Moved file should keep same UUID with inode tracking"
	);

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn test_change_detection_bulk_move_to_nested_directory() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("change_detection_bulk_move")
		.build()
		.await?;

	// Create initial location with multiple files at root
	let location = harness.create_test_location("test_location").await?;
	location.write_file("file1.txt", "Content 1").await?;
	location.write_file("file2.rs", "fn main() {}").await?;
	location.write_file("file3.md", "# Documentation").await?;
	location.write_file("file4.json", "{}").await?;

	// Initial indexing
	let handle = location.index("Test Location", IndexMode::Deep).await?;

	let initial_files = handle.count_files().await?;
	assert_eq!(initial_files, 4, "Should have 4 initial files");

	// Verify all files are at root level initially
	let entries_before = handle.get_all_entries().await?;
	let file1_before = entries_before
		.iter()
		.find(|e| e.name == "file1")
		.expect("file1 should exist");
	let file2_before = entries_before
		.iter()
		.find(|e| e.name == "file2")
		.expect("file2 should exist");

	// Store inodes and UUIDs to verify move (not delete+create)
	let inode1 = file1_before.inode;
	let uuid1 = file1_before.uuid;
	let inode2 = file2_before.inode;
	let uuid2 = file2_before.uuid;

	// Create nested directory structure and move multiple files
	handle
		.move_file("file1.txt", "archive/2024/file1.txt")
		.await?;
	handle
		.move_file("file2.rs", "archive/2024/file2.rs")
		.await?;
	handle
		.move_file("file3.md", "archive/2024/file3.md")
		.await?;

	// Re-index to detect moves
	handle.reindex().await?;

	// Verify final state
	let final_files = handle.count_files().await?;
	println!("DEBUG: final_files = {}", final_files);
	assert_eq!(final_files, 4, "Should still have 4 files after moving");

	let entries_after = handle.get_all_entries().await?;
	println!("DEBUG: entries_after count = {}", entries_after.len());
	for entry in &entries_after {
		println!(
			"DEBUG: entry: name={}, kind={}, inode={:?}, uuid={:?}",
			entry.name, entry.kind, entry.inode, entry.uuid
		);
	}

	// Verify moved files exist with new names in nested directory
	let file1_after = entries_after
		.iter()
		.find(|e| e.name == "file1")
		.expect("file1 should exist after move");
	let file2_after = entries_after
		.iter()
		.find(|e| e.name == "file2")
		.expect("file2 should exist after move");
	let _file3_after = entries_after
		.iter()
		.find(|e| e.name == "file3")
		.expect("file3 should exist after move");

	// Verify inodes and UUIDs are preserved (proves move, not delete+create)
	println!(
		"DEBUG: file1 - before inode={:?}, after inode={:?}",
		inode1, file1_after.inode
	);
	println!(
		"DEBUG: file1 - before uuid={:?}, after uuid={:?}",
		uuid1, file1_after.uuid
	);
	println!(
		"DEBUG: file2 - before inode={:?}, after inode={:?}",
		inode2, file2_after.inode
	);
	println!(
		"DEBUG: file2 - before uuid={:?}, after uuid={:?}",
		uuid2, file2_after.uuid
	);

	assert_eq!(
		inode1, file1_after.inode,
		"file1 inode should be preserved after move (before={:?}, after={:?})",
		inode1, file1_after.inode
	);
	assert_eq!(
		uuid1, file1_after.uuid,
		"file1 UUID should be preserved after move with inode tracking (before={:?}, after={:?})",
		uuid1, file1_after.uuid
	);
	assert_eq!(
		inode2, file2_after.inode,
		"file2 inode should be preserved after move (before={:?}, after={:?})",
		inode2, file2_after.inode
	);
	assert_eq!(
		uuid2, file2_after.uuid,
		"file2 UUID should be preserved after move with inode tracking (before={:?}, after={:?})",
		uuid2, file2_after.uuid
	);

	// Verify file4 remained at root
	let file4_exists = entries_after.iter().any(|e| e.name == "file4");
	assert!(file4_exists, "file4 should still exist at root");

	// Verify the nested directory structure exists
	let archive_dir = entries_after
		.iter()
		.find(|e| e.name == "archive" && e.kind == 1);
	assert!(archive_dir.is_some(), "archive directory should exist");

	let year_dir = entries_after
		.iter()
		.find(|e| e.name == "2024" && e.kind == 1);
	assert!(year_dir.is_some(), "2024 directory should exist");

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn test_shallow_vs_deep_indexing() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("shallow_vs_deep")
		.build()
		.await?;

	// Create location with same files for both modes
	let location_shallow = harness.create_test_location("shallow").await?;
	location_shallow.write_file("test.txt", "content").await?;

	let location_deep = harness.create_test_location("deep").await?;
	location_deep.write_file("test.txt", "content").await?;

	// Index with shallow mode
	let handle_shallow = location_shallow
		.index("Shallow Location", IndexMode::Shallow)
		.await?;

	// Index with deep mode
	let handle_deep = location_deep
		.index("Deep Location", IndexMode::Deep)
		.await?;

	// Both should index the file
	assert_eq!(handle_shallow.count_files().await?, 1);
	assert_eq!(handle_deep.count_files().await?, 1);

	// Deep mode should generate content identities (tested in content hash tests)
	// For now just verify both modes complete successfully

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn test_uuid_persistence_with_inode_tracking() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("uuid_persistence")
		.build()
		.await?;

	// Create location with files
	let location = harness.create_test_location("test_location").await?;
	location.write_file("file1.txt", "Content 1").await?;
	location.write_file("file2.rs", "fn main() {}").await?;
	location.create_dir("subdir").await?;
	location.write_file("subdir/file3.md", "# Test").await?;

	// Initial indexing
	let handle = location.index("Test Location", IndexMode::Deep).await?;

	// Capture all UUIDs after initial indexing
	let entries_initial = handle.get_all_entries().await?;
	let initial_uuids: std::collections::HashMap<String, Option<uuid::Uuid>> = entries_initial
		.iter()
		.map(|e| (e.name.clone(), e.uuid))
		.collect();

	// Reindex multiple times without any changes
	for i in 1..=3 {
		handle.reindex().await?;

		let entries = handle.get_all_entries().await?;

		// Verify all UUIDs remain the same
		for entry in &entries {
			let initial_uuid = initial_uuids
				.get(&entry.name)
				.unwrap_or_else(|| panic!("Entry {} should exist in initial index", entry.name));

			assert_eq!(
				initial_uuid, &entry.uuid,
				"Entry '{}' UUID should be preserved after reindex #{} (inode tracking)",
				entry.name, i
			);
		}
	}

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn test_indexing_error_handling() -> Result<()> {
	let harness = IndexingHarnessBuilder::new("error_handling")
		.build()
		.await?;

	// Try to index non-existent location
	let non_existent = harness.temp_path().join("does_not_exist");

	let result = harness
		.add_and_index_location(&non_existent, "Non-existent", IndexMode::Deep)
		.await;

	// Should fail gracefully
	assert!(
		result.is_err(),
		"Should fail to create location for non-existent path"
	);

	harness.shutdown().await?;
	Ok(())
}
