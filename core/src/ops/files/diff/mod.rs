use crate::{
	context::CoreContext,
	domain::{
		addressing::{SdPath, SdPathBatch},
		content_identity::ContentHashGenerator,
	},
	infra::query::{LibraryQuery, QueryError, QueryResult},
	ops::{
		files::copy::input::{CopyMethod, FileCopyInput},
		indexing::{
			database_storage::EntryMetadata, state::EntryKind, IndexScope, IndexerJob,
			IndexerJobConfig,
		},
	},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
	collections::HashMap,
	path::{Path, PathBuf},
	sync::Arc,
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PathDiffInput {
	pub source: SdPath,
	pub target: SdPath,
	#[serde(default)]
	pub strategy: DiffStrategy,
	#[serde(default)]
	pub use_index_rules: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum DiffStrategy {
	#[default]
	Heuristic,
	Content,
	Hybrid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DiffEntry {
	pub relative_path: PathBuf,
	pub sd_path: SdPath,
	pub uuid: Option<Uuid>,
	pub size: u64,
	pub modified_at: Option<DateTime<Utc>>,
	pub content_id: Option<String>,
	pub kind: DiffEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum DiffEntryKind {
	File,
	Directory,
	Symlink,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PathDiffResult {
	pub only_in_source: Vec<DiffEntry>,
	pub only_in_target: Vec<DiffEntry>,
	pub modified: Vec<DiffEntry>,
	pub matched_count: usize,
	pub copy_size: u64,
	pub total_scanned: usize,
}

impl PathDiffResult {
	/// Build a copy input for entries that exist only in the source tree.
	pub fn missing_copy_input(&self, destination: SdPath) -> FileCopyInput {
		let mut entries = self.only_in_source.iter().collect::<Vec<_>>();
		entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

		let mut selected_parent: Option<PathBuf> = None;
		let mut sources = Vec::new();

		for entry in entries {
			if selected_parent
				.as_ref()
				.is_some_and(|parent| entry.relative_path.starts_with(parent))
			{
				continue;
			}

			selected_parent = Some(entry.relative_path.clone());
			sources.push(entry.sd_path.clone());
		}

		FileCopyInput {
			sources: SdPathBatch { paths: sources },
			destination,
			overwrite: false,
			verify_checksum: false,
			preserve_timestamps: true,
			move_files: false,
			copy_method: CopyMethod::Auto,
			on_conflict: None,
		}
	}
}

pub struct PathDiffQuery {
	input: PathDiffInput,
}

impl LibraryQuery for PathDiffQuery {
	type Input = PathDiffInput;
	type Output = PathDiffResult;

	fn from_input(input: Self::Input) -> QueryResult<Self> {
		Ok(Self { input })
	}

	async fn execute(
		self,
		context: Arc<CoreContext>,
		session: crate::infra::api::SessionContext,
	) -> QueryResult<Self::Output> {
		let library_id = session
			.current_library_id
			.ok_or_else(|| QueryError::Internal("No library in session".to_string()))?;
		let library = context
			.libraries()
			.await
			.get_library(library_id)
			.await
			.ok_or_else(|| QueryError::Internal("Library not found".to_string()))?;

		let source = resolve_local_path(&self.input.source, &context).await?;
		let target = resolve_local_path(&self.input.target, &context).await?;

		ensure_indexed(&library, &source, self.input.use_index_rules).await?;
		ensure_indexed(&library, &target, self.input.use_index_rules).await?;

		let cache = library.core_context().ephemeral_cache();
		let source_index = cache.get_for_path(&source).ok_or_else(|| {
			QueryError::Internal(format!("Path not indexed: {}", source.display()))
		})?;
		let target_index = cache.get_for_path(&target).ok_or_else(|| {
			QueryError::Internal(format!("Path not indexed: {}", target.display()))
		})?;

		let source_map = {
			let mut map = {
				let index = source_index.read().await;
				build_path_map(&source, &index.entries(), |path| index.get_entry_uuid(path))
			};
			populate_content_ids(&mut map, self.input.strategy).await?;
			map
		};
		let target_map = {
			let mut map = {
				let index = target_index.read().await;
				build_path_map(&target, &index.entries(), |path| index.get_entry_uuid(path))
			};
			populate_content_ids(&mut map, self.input.strategy).await?;
			map
		};

		Ok(diff_path_maps(
			&source_map,
			&target_map,
			self.input.strategy,
		))
	}
}

async fn resolve_local_path(path: &SdPath, context: &Arc<CoreContext>) -> QueryResult<PathBuf> {
	let resolved = path
		.resolve(context)
		.await
		.map_err(|e| QueryError::Internal(format!("Failed to resolve path: {}", e)))?;

	let local_path = resolved
		.as_local_path()
		.map(Path::to_path_buf)
		.ok_or_else(|| {
			QueryError::Internal("Path diff currently requires local paths".to_string())
		})?;

	tokio::fs::canonicalize(&local_path).await.map_err(|e| {
		QueryError::Internal(format!(
			"Failed to canonicalize path {}: {}",
			local_path.display(),
			e
		))
	})
}

async fn ensure_indexed(
	library: &crate::library::Library,
	path: &Path,
	use_index_rules: bool,
) -> QueryResult<()> {
	let cache = library.core_context().ephemeral_cache();

	if use_index_rules && cache.is_indexed_with_scope(path, IndexScope::Recursive) {
		return Ok(());
	}

	let config = if use_index_rules {
		IndexerJobConfig::ephemeral_browse(
			SdPath::local(path.to_path_buf()),
			IndexScope::Recursive,
			false,
		)
	} else {
		IndexerJobConfig::complete_scan(SdPath::local(path.to_path_buf()), IndexScope::Recursive)
	};

	let index = cache.create_for_indexing(path.to_path_buf(), IndexScope::Recursive);
	cache.clear_for_reindex(path).await;

	let mut job = IndexerJob::new(config);
	job.set_ephemeral_index(index);

	let handle = library
		.jobs()
		.dispatch(job)
		.await
		.map_err(|e| QueryError::Internal(format!("Failed to dispatch indexer: {}", e)))?;

	handle
		.wait()
		.await
		.map_err(|e| QueryError::Internal(format!("Indexer failed: {}", e)))?;

	Ok(())
}

async fn populate_content_ids(
	entries: &mut HashMap<PathBuf, DiffEntry>,
	strategy: DiffStrategy,
) -> QueryResult<()> {
	if !strategy.requires_content_id() {
		return Ok(());
	}

	let mut futures = Vec::new();
	for (key, entry) in entries.iter() {
		if entry.kind != DiffEntryKind::File || entry.content_id.is_some() {
			continue;
		}

		if let Some(path) = entry.sd_path.as_local_path() {
			let path_buf = path.to_path_buf();
			let key_clone = key.clone();
			futures.push(async move {
				let result = ContentHashGenerator::generate_content_hash(&path_buf).await;
				(key_clone, path_buf, result)
			});
		}
	}

	let results = futures::future::join_all(futures).await;
	for (key, path, result) in results {
		match result {
			Ok(content_id) => {
				if let Some(entry) = entries.get_mut(&key) {
					entry.content_id = Some(content_id);
				}
			}
			Err(crate::domain::ContentHashError::EmptyFile) => {}
			Err(error) => {
				return Err(QueryError::Internal(format!(
					"Failed to hash {} for path diff: {}",
					path.display(),
					error
				)));
			}
		}
	}

	Ok(())
}

fn build_path_map<F>(
	root: &Path,
	entries: &HashMap<PathBuf, EntryMetadata>,
	uuid_for_path: F,
) -> HashMap<PathBuf, DiffEntry>
where
	F: Fn(&PathBuf) -> Option<Uuid>,
{
	let mut map = HashMap::new();

	for (absolute_path, metadata) in entries {
		if absolute_path == root {
			continue;
		}

		let Ok(relative_path) = absolute_path.strip_prefix(root) else {
			continue;
		};

		if relative_path.as_os_str().is_empty() {
			continue;
		}

		map.insert(
			relative_path.to_path_buf(),
			build_diff_entry(
				relative_path,
				absolute_path,
				metadata,
				uuid_for_path(absolute_path),
			),
		);
	}

	map
}

fn build_diff_entry(
	relative_path: &Path,
	absolute_path: &Path,
	metadata: &EntryMetadata,
	uuid: Option<Uuid>,
) -> DiffEntry {
	DiffEntry {
		relative_path: relative_path.to_path_buf(),
		sd_path: SdPath::local(absolute_path.to_path_buf()),
		uuid,
		size: metadata.size,
		modified_at: metadata.modified.map(DateTime::<Utc>::from),
		content_id: None,
		kind: DiffEntryKind::from(metadata.kind),
	}
}

fn diff_path_maps(
	source_map: &HashMap<PathBuf, DiffEntry>,
	target_map: &HashMap<PathBuf, DiffEntry>,
	strategy: DiffStrategy,
) -> PathDiffResult {
	match strategy {
		DiffStrategy::Heuristic => diff_heuristic(source_map, target_map),
		DiffStrategy::Content => diff_content(source_map, target_map),
		DiffStrategy::Hybrid => diff_hybrid(source_map, target_map),
	}
}

fn diff_heuristic(
	source_map: &HashMap<PathBuf, DiffEntry>,
	target_map: &HashMap<PathBuf, DiffEntry>,
) -> PathDiffResult {
	let mut result = PathDiffResult {
		total_scanned: source_map.len() + target_map.len(),
		..Default::default()
	};

	for (relative_path, source_entry) in source_map {
		match target_map.get(relative_path) {
			None => result.only_in_source.push(source_entry.clone()),
			Some(target_entry) if entries_match_heuristically(source_entry, target_entry) => {
				result.matched_count += 1;
			}
			Some(_) => result.modified.push(source_entry.clone()),
		}
	}

	for (relative_path, target_entry) in target_map {
		if !source_map.contains_key(relative_path) {
			result.only_in_target.push(target_entry.clone());
		}
	}

	sort_entries(&mut result.only_in_source);
	sort_entries(&mut result.only_in_target);
	sort_entries(&mut result.modified);

	result.copy_size = result
		.only_in_source
		.iter()
		.chain(result.modified.iter())
		.map(|entry| entry.size)
		.sum();

	result
}

fn diff_content(
	source_map: &HashMap<PathBuf, DiffEntry>,
	target_map: &HashMap<PathBuf, DiffEntry>,
) -> PathDiffResult {
	diff_with_content(source_map, target_map, false)
}

fn diff_hybrid(
	source_map: &HashMap<PathBuf, DiffEntry>,
	target_map: &HashMap<PathBuf, DiffEntry>,
) -> PathDiffResult {
	diff_with_content(source_map, target_map, true)
}

fn diff_with_content(
	source_map: &HashMap<PathBuf, DiffEntry>,
	target_map: &HashMap<PathBuf, DiffEntry>,
	heuristic_fast_path: bool,
) -> PathDiffResult {
	let mut result = PathDiffResult {
		total_scanned: source_map.len() + target_map.len(),
		..Default::default()
	};
	let target_by_content = entries_by_content_id(target_map);
	let mut consumed_targets = std::collections::HashSet::new();

	for (relative_path, source_entry) in source_map {
		if let Some(target_entry) = target_map.get(relative_path) {
			consumed_targets.insert(relative_path.clone());

			if entries_match_by_strategy(source_entry, target_entry, heuristic_fast_path) {
				result.matched_count += 1;
			} else {
				result.modified.push(source_entry.clone());
			}
			continue;
		}

		if let Some(target_relative_path) =
			find_unconsumed_content_match(source_entry, &target_by_content, &consumed_targets)
		{
			consumed_targets.insert(target_relative_path.clone());
			result.matched_count += 1;
		} else {
			result.only_in_source.push(source_entry.clone());
		}
	}

	for (relative_path, target_entry) in target_map {
		if !consumed_targets.contains(relative_path) {
			result.only_in_target.push(target_entry.clone());
		}
	}

	sort_entries(&mut result.only_in_source);
	sort_entries(&mut result.only_in_target);
	sort_entries(&mut result.modified);

	result.copy_size = result
		.only_in_source
		.iter()
		.chain(result.modified.iter())
		.map(|entry| entry.size)
		.sum();

	result
}

fn entries_by_content_id(entries: &HashMap<PathBuf, DiffEntry>) -> HashMap<String, Vec<PathBuf>> {
	let mut by_content: HashMap<String, Vec<PathBuf>> = HashMap::new();

	for (relative_path, entry) in entries {
		if entry.kind != DiffEntryKind::File {
			continue;
		}

		if let Some(content_id) = &entry.content_id {
			by_content
				.entry(content_id.clone())
				.or_default()
				.push(relative_path.clone());
		}
	}

	for paths in by_content.values_mut() {
		paths.sort();
	}

	by_content
}

fn find_unconsumed_content_match<'a>(
	source_entry: &DiffEntry,
	target_by_content: &'a HashMap<String, Vec<PathBuf>>,
	consumed_targets: &std::collections::HashSet<PathBuf>,
) -> Option<&'a PathBuf> {
	if source_entry.kind != DiffEntryKind::File {
		return None;
	}

	let content_id = source_entry.content_id.as_ref()?;
	target_by_content
		.get(content_id)?
		.iter()
		.find(|relative_path| !consumed_targets.contains(*relative_path))
}

fn entries_match_by_strategy(
	source: &DiffEntry,
	target: &DiffEntry,
	heuristic_fast_path: bool,
) -> bool {
	if heuristic_fast_path && entries_match_heuristically(source, target) {
		return true;
	}

	match (&source.content_id, &target.content_id) {
		(Some(source_content), Some(target_content)) => {
			source.kind == target.kind && source_content == target_content
		}
		_ => entries_match_heuristically(source, target),
	}
}

fn entries_match_heuristically(source: &DiffEntry, target: &DiffEntry) -> bool {
	source.kind == target.kind
		&& source.size == target.size
		&& source.modified_at == target.modified_at
}

fn sort_entries(entries: &mut [DiffEntry]) {
	entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
}

impl DiffStrategy {
	fn requires_content_id(self) -> bool {
		matches!(self, Self::Content | Self::Hybrid)
	}
}

impl From<EntryKind> for DiffEntryKind {
	fn from(kind: EntryKind) -> Self {
		match kind {
			EntryKind::File => Self::File,
			EntryKind::Directory => Self::Directory,
			EntryKind::Symlink => Self::Symlink,
		}
	}
}

crate::register_library_query!(PathDiffQuery, "files.path_diff");

#[cfg(test)]
mod tests {
	use super::*;

	fn entry(relative_path: &str, size: u64, modified_at: i64) -> DiffEntry {
		DiffEntry {
			relative_path: PathBuf::from(relative_path),
			sd_path: SdPath::local(PathBuf::from("/source").join(relative_path)),
			uuid: None,
			size,
			modified_at: DateTime::from_timestamp(modified_at, 0),
			content_id: None,
			kind: DiffEntryKind::File,
		}
	}

	fn entry_with_content(
		relative_path: &str,
		size: u64,
		modified_at: i64,
		content_id: &str,
	) -> DiffEntry {
		DiffEntry {
			content_id: Some(content_id.to_string()),
			..entry(relative_path, size, modified_at)
		}
	}

	#[test]
	fn heuristic_diff_finds_missing_modified_and_extra_entries() {
		let source_map = HashMap::from([
			(PathBuf::from("same.txt"), entry("same.txt", 10, 1)),
			(PathBuf::from("missing.txt"), entry("missing.txt", 20, 2)),
			(PathBuf::from("changed.txt"), entry("changed.txt", 30, 3)),
		]);
		let target_map = HashMap::from([
			(PathBuf::from("same.txt"), entry("same.txt", 10, 1)),
			(PathBuf::from("changed.txt"), entry("changed.txt", 31, 3)),
			(PathBuf::from("extra.txt"), entry("extra.txt", 40, 4)),
		]);

		let result = diff_heuristic(&source_map, &target_map);

		assert_eq!(result.matched_count, 1);
		assert_eq!(result.total_scanned, 6);
		assert_eq!(result.copy_size, 50);
		assert_eq!(
			result.only_in_source[0].relative_path,
			PathBuf::from("missing.txt")
		);
		assert_eq!(
			result.modified[0].relative_path,
			PathBuf::from("changed.txt")
		);
		assert_eq!(
			result.only_in_target[0].relative_path,
			PathBuf::from("extra.txt")
		);
	}

	#[test]
	fn heuristic_diff_sorts_entries_by_relative_path() {
		let source_map = HashMap::from([
			(PathBuf::from("z.txt"), entry("z.txt", 1, 1)),
			(PathBuf::from("a.txt"), entry("a.txt", 1, 1)),
		]);
		let target_map = HashMap::new();

		let result = diff_heuristic(&source_map, &target_map);

		assert_eq!(
			result.only_in_source[0].relative_path,
			PathBuf::from("a.txt")
		);
		assert_eq!(
			result.only_in_source[1].relative_path,
			PathBuf::from("z.txt")
		);
	}

	#[test]
	fn content_diff_matches_renamed_files_by_hash() {
		let source_map = HashMap::from([(
			PathBuf::from("photos/original.jpg"),
			entry_with_content("photos/original.jpg", 10, 1, "same-content"),
		)]);
		let target_map = HashMap::from([(
			PathBuf::from("archive/renamed.jpg"),
			entry_with_content("archive/renamed.jpg", 10, 2, "same-content"),
		)]);

		let result = diff_content(&source_map, &target_map);

		assert_eq!(result.matched_count, 1);
		assert_eq!(result.copy_size, 0);
		assert!(result.only_in_source.is_empty());
		assert!(result.only_in_target.is_empty());
		assert!(result.modified.is_empty());
	}

	#[test]
	fn content_diff_marks_same_path_different_hash_as_modified() {
		let source_map = HashMap::from([(
			PathBuf::from("report.txt"),
			entry_with_content("report.txt", 10, 1, "source-content"),
		)]);
		let target_map = HashMap::from([(
			PathBuf::from("report.txt"),
			entry_with_content("report.txt", 10, 1, "target-content"),
		)]);

		let result = diff_content(&source_map, &target_map);

		assert_eq!(result.matched_count, 0);
		assert_eq!(result.copy_size, 10);
		assert_eq!(
			result.modified[0].relative_path,
			PathBuf::from("report.txt")
		);
		assert!(result.only_in_source.is_empty());
		assert!(result.only_in_target.is_empty());
	}

	#[test]
	fn hybrid_diff_uses_heuristic_fast_path_for_same_metadata() {
		let source_map = HashMap::from([(
			PathBuf::from("same.txt"),
			entry_with_content("same.txt", 10, 1, "source-content"),
		)]);
		let target_map = HashMap::from([(
			PathBuf::from("same.txt"),
			entry_with_content("same.txt", 10, 1, "target-content"),
		)]);

		let result = diff_hybrid(&source_map, &target_map);

		assert_eq!(result.matched_count, 1);
		assert_eq!(result.copy_size, 0);
		assert!(result.modified.is_empty());
	}

	#[test]
	fn hybrid_diff_falls_back_to_content_for_renames() {
		let source_map = HashMap::from([(
			PathBuf::from("draft.txt"),
			entry_with_content("draft.txt", 10, 1, "same-content"),
		)]);
		let target_map = HashMap::from([(
			PathBuf::from("published.txt"),
			entry_with_content("published.txt", 10, 9, "same-content"),
		)]);

		let result = diff_hybrid(&source_map, &target_map);

		assert_eq!(result.matched_count, 1);
		assert!(result.only_in_source.is_empty());
		assert!(result.only_in_target.is_empty());
	}

	#[test]
	fn missing_copy_input_uses_only_source_entries() {
		let result = PathDiffResult {
			only_in_source: vec![entry("missing.txt", 10, 1)],
			modified: vec![entry("changed.txt", 20, 2)],
			..Default::default()
		};

		let input = result.missing_copy_input(SdPath::local("/target"));

		assert_eq!(input.sources.paths.len(), 1);
		assert_eq!(input.sources.paths[0], SdPath::local("/source/missing.txt"));
		assert_eq!(input.destination, SdPath::local("/target"));
	}

	#[test]
	fn missing_copy_input_skips_children_when_parent_is_selected() {
		let result = PathDiffResult {
			only_in_source: vec![
				entry("photos", 0, 1),
				entry("photos/image.jpg", 10, 1),
				entry("z.txt", 1, 1),
			],
			..Default::default()
		};

		let input = result.missing_copy_input(SdPath::local("/target"));

		assert_eq!(
			input.sources.paths,
			vec![
				SdPath::local("/source/photos"),
				SdPath::local("/source/z.txt")
			]
		);
	}
}
