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
		files::query::{
			AlternateInstancesQuery, DirectoryListingQuery, FileByIdQuery, FileByPathQuery,
			UniqueToLocationQuery,
		},
		metadata::set_favorite::{action::SetFavoriteAction, input::SetFavoriteInput},
		search::{
			input::{
				FileSearchInput, PaginationOptions, SearchFilters, SearchMode, SearchScope,
				SortDirection, SortField, SortOptions,
			},
			query::FileSearchQuery,
		},
		tags::{
			apply::{action::ApplyTagsAction, input::ApplyTagsInput},
			create::{action::CreateTagAction, input::CreateTagInput},
			files_by_tag::{GetFilesByTagInput, GetFilesByTagQuery},
		},
	},
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
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

/// Wait until content identification has linked a content identity to `entry_uuid`.
///
/// Discovery inserts entries with `content_id = NULL`; a later indexing phase
/// hashes the bytes and fills it in (only for `IndexMode >= Content`).
/// `files.unique_to_location` joins through that column and
/// `files.alternate_instances` refuses to run without it, so querying too early
/// yields an empty result that looks exactly like a favorites bug. Waiting here
/// keeps a slow content phase from being misread as a missing lookup.
async fn wait_for_content_id(
	harness: &IndexingHarness,
	entry_uuid: uuid::Uuid,
	timeout: Duration,
) -> anyhow::Result<()> {
	let db = harness.library.db().conn();
	let deadline = tokio::time::Instant::now() + timeout;

	loop {
		let linked = entry::Entity::find()
			.filter(entry::Column::Uuid.eq(entry_uuid))
			.one(db)
			.await?
			.is_some_and(|model| model.content_id.is_some());

		if linked {
			return Ok(());
		}

		if tokio::time::Instant::now() >= deadline {
			let with_content = entry::Entity::find()
				.filter(entry::Column::ContentId.is_not_null())
				.count(db)
				.await?;
			anyhow::bail!(
				"entry {entry_uuid} still has no content_id after {timeout:?} \
				 ({with_content} entr(ies) in the library have one). Both \
				 files.unique_to_location and files.alternate_instances resolve files \
				 through content identity, so neither can return this file until the \
				 indexer's content phase has run."
			);
		}

		tokio::time::sleep(Duration::from_millis(50)).await;
	}
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

	let unique = UniqueToLocationQuery::new(location_uuid)
		.execute(harness.core.context.clone(), session_for(harness))
		.await?;
	let unique_hit = unique
		.unique_files
		.iter()
		.find(|f| f.name == file_stem)
		.ok_or_else(|| {
			anyhow::anyhow!(
				"{file_stem} missing from files.unique_to_location, which returned {} file(s); \
				 the harness indexes a single location, so every file in it is unique to it",
				unique.unique_files.len()
			)
		})?;
	observed.push(("files.unique_to_location", unique_hit.favorite));

	// Returns every entry sharing this file's content identity, so the file itself
	// comes back even with no duplicates present.
	let alternates = AlternateInstancesQuery::new(entry_uuid)
		.execute(harness.core.context.clone(), session_for(harness))
		.await?;
	let alternate_hit = alternates
		.instances
		.iter()
		.find(|f| f.name == file_stem)
		.ok_or_else(|| {
			anyhow::anyhow!(
				"{file_stem} missing from files.alternate_instances, which returned {} \
				 instance(s); the file must list itself among its own instances",
				alternates.instances.len()
			)
		})?;
	observed.push(("files.alternate_instances", alternate_hit.favorite));

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
	// Must track the number of paths in `favorite_across_paths`. Without it a path
	// that stops returning the file drops silently out of `observed` and this test
	// keeps passing while covering less than it claims.
	assert!(
		observed.len() >= 6,
		"{stage}: only {} query paths were exercised, expected 6; the coverage this test \
		 claims is not real",
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
	wait_for_content_id(&harness, entry_uuid, Duration::from_secs(10)).await?;
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
async fn favorite_does_not_leak_between_content_identical_files() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("favorite_content_identity")
		.build()
		.await?;

	let test_location = harness.create_test_location("favorites_duplicates").await?;
	// Byte-identical, so both entries end up on one content identity. Favorite is
	// keyed by entry uuid but tags on this same path are resolved content-scoped,
	// so a favorite lookup that drifted onto the content key would light up both.
	test_location
		.write_file("original.txt", "identical bytes")
		.await?;
	test_location
		.write_file("duplicate.txt", "identical bytes")
		.await?;

	test_location.index("Duplicates", IndexMode::Deep).await?;
	tokio::time::sleep(Duration::from_millis(500)).await;

	let entry_uuid = entry_uuid_for(&harness, "original").await?;
	wait_for_content_id(&harness, entry_uuid, Duration::from_secs(10)).await?;

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

	let alternates = AlternateInstancesQuery::new(entry_uuid)
		.execute(harness.core.context.clone(), session_for(&harness))
		.await?;

	// Without this the per-file assertion below passes vacuously: if the two files
	// never shared a content identity the query returns just the original, and a
	// leak between instances could not be observed at all.
	assert!(
		alternates.instances.len() >= 2,
		"files.alternate_instances returned {} instance(s); the two files have identical \
		 bytes and must share one content identity for this test to prove anything",
		alternates.instances.len()
	);

	for file in &alternates.instances {
		let expected = file.name == "original";
		assert_eq!(
			file.favorite, expected,
			"{} should have favorite={expected}; only the favorited entry may report true, \
			 even though every instance here shares one content identity",
			file.name
		);
	}

	harness.shutdown().await?;
	Ok(())
}

/// `files.by_tag` is the tag-sidebar's listing path.
///
/// It builds its `File`s from a tag join rather than a directory walk, so it
/// resolves `favorite` through its own `File::favorite_entry_uuids` call. It
/// can't be folded into `favorite_across_paths` because it only returns files
/// that carry a tag, which the shared fixture deliberately doesn't set up.
#[tokio::test]
async fn favorite_survives_the_tag_listing_path() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("favorite_by_tag").build().await?;

	let test_location = harness.create_test_location("favorites_tagged").await?;
	test_location.write_file("tagged.txt", "favorited and tagged").await?;
	test_location.write_file("untagged.txt", "neither").await?;

	test_location.index("Tagged", IndexMode::Deep).await?;
	tokio::time::sleep(Duration::from_millis(500)).await;

	let db = harness.library.db().conn();
	let entry_model = entry::Entity::find()
		.filter(entry::Column::Name.eq("tagged"))
		.one(db)
		.await?
		.ok_or_else(|| anyhow::anyhow!("no indexed entry named tagged"))?;
	let entry_uuid = entry_model
		.uuid
		.ok_or_else(|| anyhow::anyhow!("entry tagged exists but has no uuid"))?;

	let action_manager = harness.core.context.get_action_manager().await.unwrap();
	let library_id = harness.library.id();

	let tag = action_manager
		.dispatch_library(
			Some(library_id),
			CreateTagAction::from_input(CreateTagInput::simple("Keepers".to_string())).unwrap(),
		)
		.await?;

	action_manager
		.dispatch_library(
			Some(library_id),
			ApplyTagsAction::from_input(ApplyTagsInput::user_tags_entry(
				vec![entry_model.id],
				vec![tag.tag_id],
			))
			.unwrap(),
		)
		.await?;

	// Applying a tag writes the same user_metadata row that favorite lives on, so
	// read the flag back before setting it: if tagging alone flipped it, the
	// assertion after favoriting would pass for the wrong reason.
	let by_tag = |harness: &IndexingHarness| {
		GetFilesByTagQuery::from_input(GetFilesByTagInput {
			tag_id: tag.tag_id,
			include_children: false,
			min_confidence: 0.0,
		})
		.unwrap()
		.execute(harness.core.context.clone(), session_for(harness))
	};

	let tagged_only = by_tag(&harness).await?;
	let hit = tagged_only
		.files
		.iter()
		.find(|f| f.name == "tagged")
		.ok_or_else(|| {
			anyhow::anyhow!(
				"tagged missing from files.by_tag, which returned {} file(s); the tag was \
				 just applied to this entry",
				tagged_only.files.len()
			)
		})?;
	assert!(
		!hit.favorite,
		"files.by_tag reports favorite=true before anything was favorited; applying a tag \
		 must not set the favorite flag on the shared user_metadata row"
	);

	action_manager
		.dispatch_library(
			Some(library_id),
			SetFavoriteAction::from_input(SetFavoriteInput {
				entry_uuid,
				favorite: true,
			})
			.unwrap(),
		)
		.await?;

	let after = by_tag(&harness).await?;
	let hit = after
		.files
		.iter()
		.find(|f| f.name == "tagged")
		.ok_or_else(|| anyhow::anyhow!("tagged disappeared from files.by_tag after favoriting"))?;
	assert!(
		hit.favorite,
		"files.by_tag reports favorite=false after the flag was set; the tag sidebar would \
		 show this file as un-favorited while every other view shows it favorited"
	);

	// Un-favoriting must propagate here too.
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

	let cleared = by_tag(&harness).await?;
	let hit = cleared
		.files
		.iter()
		.find(|f| f.name == "tagged")
		.ok_or_else(|| anyhow::anyhow!("tagged disappeared from files.by_tag after clearing"))?;
	assert!(
		!hit.favorite,
		"files.by_tag still reports favorite=true after it was cleared"
	);

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
