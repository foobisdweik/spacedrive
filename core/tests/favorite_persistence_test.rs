//! Integration tests for persistent file favorites.
//!
//! `metadata.set_favorite` writes one row, but ten separate query paths each
//! resolve `File::favorite` independently via `File::favorite_entry_uuids`.
//! Nothing forces them to agree, so a path that forgets the lookup returns
//! `favorite: false` and the flag simply appears not to have saved — on that
//! screen only. These tests set the flag once and read it back through every
//! path, so a missed lookup fails here instead of being found by whoever
//! happens to open the right view.

mod helpers;

use helpers::*;
use sd_core::{
	domain::addressing::SdPath,
	infra::{
		action::LibraryAction,
		api::SessionContext,
		db::entities::{entry, user_metadata},
		query::LibraryQuery,
	},
	location::IndexMode,
	ops::{
		files::query::{DirectoryListingQuery, FileByIdQuery, FileByPathQuery},
		metadata::set_favorite::{action::SetFavoriteAction, input::SetFavoriteInput},
		search::{
			input::{
				FileSearchInput, PaginationOptions, SearchFilters, SearchMode, SearchScope,
				SortDirection, SortField, SortOptions,
			},
			query::FileSearchQuery,
		},
	},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use std::path::{Path, PathBuf};
use tokio::time::Duration;

fn session_for(harness: &IndexingHarness) -> SessionContext {
	let device_id = sd_core::device::get_current_device_id();
	let device_name = sd_core::device::get_current_device_slug();
	let mut session = SessionContext::device_session(device_id, device_name);
	session.current_library_id = Some(harness.library.id());
	session
}

/// Resolve the `entry.uuid` for an indexed file by name.
///
/// On a miss this dumps what the table actually holds — otherwise a lookup
/// failure is indistinguishable from "indexing wrote nothing", and the two
/// have very different causes.
async fn entry_uuid_for(harness: &IndexingHarness, name: &str) -> anyhow::Result<uuid::Uuid> {
	let db = harness.library.db().conn();

	let model = match entry::Entity::find()
		.filter(entry::Column::Name.eq(name))
		.one(db)
		.await?
	{
		Some(model) => model,
		None => {
			let all = entry::Entity::find().all(db).await?;
			let names: Vec<_> = all
				.iter()
				.map(|e| format!("{} (uuid={:?})", e.name, e.uuid))
				.collect();
			anyhow::bail!(
				"no indexed entry named {name}; entry table holds {} row(s): {:?}",
				all.len(),
				names
			);
		}
	};

	model
		.uuid
		.ok_or_else(|| anyhow::anyhow!("entry {name} exists but has no uuid"))
}

/// Read `favorite` for one file through every query path that exposes it.
/// Returns (path label, favorite) so a disagreeing path names itself on failure.
///
/// `file_stem` is the name *without* extension: `entry.name` holds the stem and
/// keeps `extension` in its own column, and `File::name` inherits that. Passing
/// a name with its extension attached matches nothing.
async fn favorite_across_paths(
	harness: &IndexingHarness,
	dir: &Path,
	file_path: &PathBuf,
	entry_uuid: uuid::Uuid,
	file_stem: &str,
	location_uuid: uuid::Uuid,
) -> anyhow::Result<Vec<(&'static str, bool)>> {
	let mut observed = Vec::new();

	let listing = DirectoryListingQuery::new(SdPath::local(dir.to_path_buf()))
		.execute(harness.core.context.clone(), session_for(harness))
		.await?;
	let listed = listing
		.files
		.iter()
		.find(|f| f.name == file_stem)
		.ok_or_else(|| anyhow::anyhow!("{file_stem} missing from directory listing"))?;
	observed.push(("files.directory_listing", listed.favorite));

	let by_id = FileByIdQuery::new(entry_uuid)
		.execute(harness.core.context.clone(), session_for(harness))
		.await?;
	observed.push((
		"files.by_id",
		by_id
			.ok_or_else(|| anyhow::anyhow!("files.by_id returned no file"))?
			.favorite,
	));

	let by_path = FileByPathQuery::new(file_path.clone())
		.execute(harness.core.context.clone(), session_for(harness))
		.await?;
	observed.push((
		"files.by_path",
		by_path
			.ok_or_else(|| anyhow::anyhow!("files.by_path returned no file"))?
			.favorite,
	));

	let search = FileSearchQuery::new(FileSearchInput {
		query: file_stem.to_string(),
		scope: SearchScope::Location {
			location_id: location_uuid,
		},
		mode: SearchMode::Normal,
		filters: SearchFilters::default(),
		sort: SortOptions {
			field: SortField::Relevance,
			direction: SortDirection::Desc,
		},
		pagination: PaginationOptions {
			limit: 50,
			offset: 0,
		},
	})
	.execute(harness.core.context.clone(), session_for(harness))
	.await?;
	if let Some(hit) = search.files.iter().find(|f| f.name == file_stem) {
		observed.push(("search.files", hit.favorite));
	}

	Ok(observed)
}

fn assert_all(observed: &[(&'static str, bool)], expected: bool, stage: &str) {
	let disagreeing: Vec<_> = observed
		.iter()
		.filter(|(_, actual)| *actual != expected)
		.map(|(path, _)| *path)
		.collect();

	assert!(
		disagreeing.is_empty(),
		"{stage}: expected favorite={expected} from every query path, but {:?} disagreed \
		 (full readout: {:?}). A path that disagrees is missing its \
		 File::favorite_entry_uuids lookup.",
		disagreeing,
		observed
	);
	assert!(
		observed.len() >= 4,
		"{stage}: only {} query paths were exercised; the coverage this test claims is not real",
		observed.len()
	);
}

#[tokio::test]
async fn favorite_is_consistent_across_every_query_path() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("favorite_persistence")
		.build()
		.await?;

	let test_location = harness.create_test_location("favorites").await?;
	let target = test_location
		.write_file("notes/keeper.txt", "the favorited one")
		.await?;
	test_location
		.write_file("notes/other.txt", "not favorited")
		.await?;

	let location = test_location.index("Favorites", IndexMode::Deep).await?;
	tokio::time::sleep(Duration::from_millis(500)).await;

	let dir = target.parent().unwrap().to_path_buf();
	let entry_uuid = entry_uuid_for(&harness, "keeper").await?;
	let action_manager = harness.core.context.get_action_manager().await.unwrap();
	let library_id = harness.library.id();

	// Baseline: nothing is favorited yet, so every path must say false.
	let before =
		favorite_across_paths(&harness, &dir, &target, entry_uuid, "keeper", location.uuid).await?;
	assert_all(&before, false, "before favoriting");

	// Favorite it.
	let output = action_manager
		.dispatch_library(
			Some(library_id),
			SetFavoriteAction::from_input(SetFavoriteInput {
				entry_uuid,
				favorite: true,
			})
			.unwrap(),
		)
		.await?;
	assert!(output.favorite, "action reported favorite=false after set");

	// The row itself must exist and be set — distinguishes "never written" from
	// "written but not surfaced" when a query path disagrees below.
	let metadata = user_metadata::Entity::find()
		.filter(user_metadata::Column::EntryUuid.eq(entry_uuid))
		.one(harness.library.db().conn())
		.await?
		.ok_or_else(|| anyhow::anyhow!("no user_metadata row written for the favorited entry"))?;
	assert!(
		metadata.favorite,
		"user_metadata row exists but favorite is false"
	);

	let after =
		favorite_across_paths(&harness, &dir, &target, entry_uuid, "keeper", location.uuid).await?;
	assert_all(&after, true, "after favoriting");

	// Un-favoriting has to propagate everywhere too; a path that caches the
	// first read would pass the set case and fail here.
	action_manager
		.dispatch_library(
			Some(library_id),
			SetFavoriteAction::from_input(SetFavoriteInput {
				entry_uuid,
				favorite: false,
			})
			.unwrap(),
		)
		.await?;

	let cleared =
		favorite_across_paths(&harness, &dir, &target, entry_uuid, "keeper", location.uuid).await?;
	assert_all(&cleared, false, "after un-favoriting");

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn favoriting_one_file_does_not_favorite_its_neighbours() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("favorite_isolation")
		.build()
		.await?;

	let test_location = harness.create_test_location("favorites_isolation").await?;
	let target = test_location.write_file("a.txt", "favorited").await?;
	test_location.write_file("b.txt", "untouched").await?;
	test_location.write_file("c.txt", "untouched").await?;

	test_location.index("Isolation", IndexMode::Deep).await?;
	tokio::time::sleep(Duration::from_millis(500)).await;

	let entry_uuid = entry_uuid_for(&harness, "a").await?;
	let action_manager = harness.core.context.get_action_manager().await.unwrap();

	action_manager
		.dispatch_library(
			Some(harness.library.id()),
			SetFavoriteAction::from_input(SetFavoriteInput {
				entry_uuid,
				favorite: true,
			})
			.unwrap(),
		)
		.await?;

	let dir = target.parent().unwrap().to_path_buf();
	let listing = DirectoryListingQuery::new(SdPath::local(dir))
		.execute(harness.core.context.clone(), session_for(&harness))
		.await?;

	for file in &listing.files {
		let expected = file.name == "a";
		assert_eq!(
			file.favorite, expected,
			"{} should have favorite={expected}; a bulk-update bug would light up the neighbours",
			file.name
		);
	}

	harness.shutdown().await?;
	Ok(())
}

#[tokio::test]
async fn favoriting_an_unindexed_entry_is_rejected() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("favorite_unindexed")
		.build()
		.await?;

	let action_manager = harness.core.context.get_action_manager().await.unwrap();

	let result = action_manager
		.dispatch_library(
			Some(harness.library.id()),
			SetFavoriteAction::from_input(SetFavoriteInput {
				entry_uuid: uuid::Uuid::new_v4(),
				favorite: true,
			})
			.unwrap(),
		)
		.await;

	assert!(
		result.is_err(),
		"favoriting an entry that was never indexed should be rejected, not silently persisted"
	);

	harness.shutdown().await?;
	Ok(())
}
