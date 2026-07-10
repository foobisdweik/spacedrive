//! Integration tests for path intersection & smart diff (FILE-006)
//!
//! Diff two directories, copy the missing entries via the diff-produced copy
//! input, and verify a re-diff reports nothing left to copy.

mod helpers;

use helpers::*;
use sd_core::{
	domain::addressing::SdPath,
	infra::{api::SessionContext, query::LibraryQuery},
	ops::files::{
		copy::job::{CopyOptions, FileCopyJob},
		diff::{DiffStrategy, PathDiffInput, PathDiffQuery, PathDiffResult},
	},
};
use std::path::Path;

async fn run_diff(
	harness: &IndexingHarness,
	source: &Path,
	target: &Path,
) -> anyhow::Result<PathDiffResult> {
	let query = PathDiffQuery::from_input(PathDiffInput {
		source: SdPath::local(source.to_path_buf()),
		target: SdPath::local(target.to_path_buf()),
		strategy: DiffStrategy::Heuristic,
		use_index_rules: false,
	})
	.map_err(|e| anyhow::anyhow!(e))?;

	let mut session = SessionContext::device_session(harness.device_id, "diff-test".to_string());
	session.current_library_id = Some(harness.library.id());

	query
		.execute(harness.core.context.clone(), session)
		.await
		.map_err(|e| anyhow::anyhow!("diff query failed: {e}"))
}

fn relative_paths(entries: &[sd_core::ops::files::diff::DiffEntry]) -> Vec<String> {
	entries
		.iter()
		.map(|e| e.relative_path.to_string_lossy().to_string())
		.collect()
}

#[tokio::test]
async fn test_diff_copy_rediff_shows_zero_missing() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("path_diff")
		.disable_watcher()
		.build()
		.await?;

	let source = harness.temp_path().join("diff_source");
	let target = harness.temp_path().join("diff_target");
	tokio::fs::create_dir_all(source.join("sub")).await?;
	tokio::fs::create_dir_all(&target).await?;
	tokio::fs::write(source.join("a.txt"), "alpha").await?;
	tokio::fs::write(source.join("sub/b.txt"), "beta").await?;
	tokio::fs::write(target.join("extra.txt"), "only here").await?;

	// 1. Initial diff: everything in source is missing from target.
	let diff = run_diff(&harness, &source, &target).await?;

	let missing = relative_paths(&diff.only_in_source);
	assert!(
		missing.iter().any(|p| p == "a.txt"),
		"a.txt must be reported missing from target, got {missing:?}"
	);
	assert!(
		missing.iter().any(|p| p.ends_with("b.txt")),
		"sub/b.txt must be reported missing from target, got {missing:?}"
	);
	let extra = relative_paths(&diff.only_in_target);
	assert!(
		extra.iter().any(|p| p == "extra.txt"),
		"extra.txt must be reported as only in target, got {extra:?}"
	);
	assert!(
		diff.copy_size > 0,
		"copy_size must account for missing files"
	);

	// 2. Copy the diff: feed the result straight into FileCopyJob.
	let copy_input = diff.missing_copy_input(SdPath::local(target.clone()));
	assert!(
		!copy_input.sources.paths.is_empty(),
		"missing_copy_input must produce sources to copy"
	);

	let copy_job =
		FileCopyJob::new(copy_input.sources, copy_input.destination).with_options(CopyOptions {
			conflict_resolution: None,
			overwrite: copy_input.overwrite,
			copy_method: copy_input.copy_method,
			verify_checksum: copy_input.verify_checksum,
			preserve_timestamps: copy_input.preserve_timestamps,
			delete_after_copy: false,
			move_mode: None,
		});
	let handle = harness.library.jobs().dispatch(copy_job).await?;
	handle.wait().await?;

	assert_eq!(
		tokio::fs::read_to_string(target.join("a.txt")).await?,
		"alpha"
	);
	assert_eq!(
		tokio::fs::read_to_string(target.join("sub/b.txt")).await?,
		"beta"
	);

	// 3. Re-diff: nothing left to copy from source.
	let rediff = run_diff(&harness, &source, &target).await?;
	assert!(
		rediff.only_in_source.is_empty(),
		"re-diff after copy must report zero missing files, got {:?}",
		relative_paths(&rediff.only_in_source)
	);

	Ok(())
}

#[tokio::test]
async fn test_diff_without_rules_sees_filtered_files() -> anyhow::Result<()> {
	let harness = IndexingHarnessBuilder::new("path_diff_no_rules")
		.disable_watcher()
		.build()
		.await?;

	let source = harness.temp_path().join("no_rules_source");
	let target = harness.temp_path().join("no_rules_target");
	tokio::fs::create_dir_all(source.join("node_modules/pkg")).await?;
	tokio::fs::create_dir_all(&target).await?;
	tokio::fs::write(source.join("node_modules/pkg/index.js"), "js").await?;
	tokio::fs::write(source.join("visible.txt"), "text").await?;

	// use_index_rules: false runs a complete scan (INDEX-011), so files that
	// indexer rules normally exclude still participate in the diff.
	let diff = run_diff(&harness, &source, &target).await?;

	let missing = relative_paths(&diff.only_in_source);
	assert!(
		missing.iter().any(|p| p.ends_with("index.js")),
		"complete scan diff must see rule-filtered files, got {missing:?}"
	);
	assert!(
		missing.iter().any(|p| p == "visible.txt"),
		"regular files must be reported too, got {missing:?}"
	);

	Ok(())
}
