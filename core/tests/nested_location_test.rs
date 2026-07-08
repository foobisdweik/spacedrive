//! Integration tests for nested locations sharing entry trees (LOC-005)
//!
//! A location is a virtual pointer to a directory entry, so a location added
//! over an already-indexed directory reuses the existing entry. These tests
//! verify that location removal respects shared entry trees:
//! 1. Adding a nested location reuses the existing directory entry.
//! 2. Removing the parent location preserves the nested location's subtree
//!    and detaches its root.
//! 3. Removing the nested location preserves the parent's entries.

mod helpers;

use helpers::*;
use sd_core::{
	infra::db::entities,
	location::{IndexMode, LocationManager},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

#[tokio::test]
async fn test_parent_location_removal_preserves_nested_location() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("nested_loc_parent_removal")
		.disable_watcher()
		.build()
		.await?;

	let test_location = harness.create_test_location("parent_root").await?;
	test_location.write_file("root_file.txt", "root").await?;
	test_location
		.write_file("sub/nested_file.txt", "nested")
		.await?;
	test_location
		.write_file("sub/deep/deep_file.txt", "deep")
		.await?;

	let parent = test_location.index("Parent", IndexMode::Shallow).await?;
	let parent_root_id = parent.entry_id.expect("parent location has root entry");

	let parent_entries = parent.get_all_entries().await?;
	let sub_entry_id = parent_entries
		.iter()
		.find(|e| e.name == "sub" && e.kind == 1)
		.expect("sub directory indexed under parent")
		.id;
	let root_file_id = parent_entries
		.iter()
		.find(|e| e.name.starts_with("root_file"))
		.expect("root_file indexed under parent")
		.id;

	// Adding a location over the already-indexed subdirectory must reuse the
	// existing entry instead of creating a duplicate root.
	let nested = harness
		.add_and_index_location(
			test_location.path().join("sub"),
			"Nested",
			IndexMode::Shallow,
		)
		.await?;
	assert_eq!(
		nested.entry_id,
		Some(sub_entry_id),
		"nested location must reuse the existing directory entry"
	);

	let nested_subtree_ids = nested.get_all_entry_ids().await?;
	assert!(
		nested_subtree_ids.len() >= 4,
		"nested subtree should contain sub, nested_file, deep, deep_file (got {})",
		nested_subtree_ids.len()
	);

	// Remove the parent location.
	let manager = LocationManager::new((*harness.core.events).clone());
	manager
		.remove_location(&harness.library, parent.uuid)
		.await?;

	let conn = harness.library.db().conn();

	// The nested location row must survive (locations.entry_id is ON DELETE
	// CASCADE, so this also proves its root entry was not deleted).
	let nested_row = entities::location::Entity::find()
		.filter(entities::location::Column::Uuid.eq(nested.uuid))
		.one(conn)
		.await?;
	assert!(
		nested_row.is_some(),
		"nested location must survive parent removal"
	);

	// The nested root entry survives and is detached to a standalone root.
	let sub_entry = entities::entry::Entity::find_by_id(sub_entry_id)
		.one(conn)
		.await?
		.expect("nested location root entry must survive parent removal");
	assert_eq!(
		sub_entry.parent_id, None,
		"nested location root must be detached from the deleted parent tree"
	);

	// The whole nested subtree survives.
	for id in &nested_subtree_ids {
		let entry = entities::entry::Entity::find_by_id(*id).one(conn).await?;
		assert!(
			entry.is_some(),
			"entry {} in nested subtree must survive parent removal",
			id
		);
	}

	// Its directory_paths cache row survives (absolute path stays valid).
	let dir_path = entities::directory_paths::Entity::find_by_id(sub_entry_id)
		.one(conn)
		.await?;
	assert!(
		dir_path.is_some(),
		"directory_paths row for nested root must survive parent removal"
	);

	// Entries covered only by the parent location are gone.
	assert!(
		entities::entry::Entity::find_by_id(parent_root_id)
			.one(conn)
			.await?
			.is_none(),
		"parent root entry must be deleted"
	);
	assert!(
		entities::entry::Entity::find_by_id(root_file_id)
			.one(conn)
			.await?
			.is_none(),
		"entries outside the nested subtree must be deleted"
	);

	harness.shutdown().await
}

#[tokio::test]
async fn test_nested_location_removal_preserves_parent_entries() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("nested_loc_nested_removal")
		.disable_watcher()
		.build()
		.await?;

	let test_location = harness.create_test_location("parent_root").await?;
	test_location.write_file("root_file.txt", "root").await?;
	test_location
		.write_file("sub/nested_file.txt", "nested")
		.await?;

	let parent = test_location.index("Parent", IndexMode::Shallow).await?;

	let parent_entries = parent.get_all_entries().await?;
	let sub_entry_id = parent_entries
		.iter()
		.find(|e| e.name == "sub" && e.kind == 1)
		.expect("sub directory indexed under parent")
		.id;

	let nested = harness
		.add_and_index_location(
			test_location.path().join("sub"),
			"Nested",
			IndexMode::Shallow,
		)
		.await?;
	assert_eq!(nested.entry_id, Some(sub_entry_id));

	// Snapshot the parent's entry set after both locations exist.
	let parent_ids_before = parent.get_all_entry_ids().await?;

	// Remove the nested location. The parent still covers the subtree, so no
	// entries may be deleted.
	let manager = LocationManager::new((*harness.core.events).clone());
	manager
		.remove_location(&harness.library, nested.uuid)
		.await?;

	let conn = harness.library.db().conn();

	let nested_row = entities::location::Entity::find()
		.filter(entities::location::Column::Uuid.eq(nested.uuid))
		.one(conn)
		.await?;
	assert!(nested_row.is_none(), "nested location row must be deleted");

	for id in &parent_ids_before {
		let entry = entities::entry::Entity::find_by_id(*id).one(conn).await?;
		assert!(
			entry.is_some(),
			"entry {} must survive nested location removal (parent still covers it)",
			id
		);
	}

	// The shared entry stays attached to the parent tree.
	let sub_entry = entities::entry::Entity::find_by_id(sub_entry_id)
		.one(conn)
		.await?
		.expect("shared entry must survive");
	assert_eq!(
		sub_entry.parent_id,
		Some(parent.entry_id.expect("parent root entry")),
		"shared entry must stay attached to the parent tree"
	);

	harness.shutdown().await
}
