//! Input types for file duplicate operations.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::domain::addressing::{SdPath, SdPathBatch};

use super::super::copy::{
	action::FileConflictResolution,
	input::{CopyMethod, FileCopyInput},
};

/// Input for duplicating files or directories beside their originals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FileDuplicateInput {
	/// Files or directories to duplicate.
	pub sources: SdPathBatch,
	/// Whether to verify checksums during duplicate copies.
	pub verify_checksum: bool,
	/// Whether to preserve file timestamps.
	pub preserve_timestamps: bool,
	/// Preferred copy method.
	pub copy_method: CopyMethod,
}

impl FileDuplicateInput {
	pub fn validate(&self) -> Result<(), Vec<String>> {
		duplicate_destination(&self.sources)
			.map(|_| ())
			.map_err(|e| vec![e])
	}

	pub fn to_copy_input(&self) -> Result<FileCopyInput, String> {
		let destination = duplicate_destination(&self.sources)?;

		Ok(FileCopyInput {
			sources: self.sources.clone(),
			destination,
			overwrite: false,
			verify_checksum: self.verify_checksum,
			preserve_timestamps: self.preserve_timestamps,
			move_files: false,
			copy_method: self.copy_method.clone(),
			on_conflict: Some(FileConflictResolution::AutoModifyName),
		})
	}
}

impl Default for FileDuplicateInput {
	fn default() -> Self {
		Self {
			sources: SdPathBatch { paths: Vec::new() },
			verify_checksum: false,
			preserve_timestamps: true,
			copy_method: CopyMethod::Auto,
		}
	}
}

pub(crate) fn duplicate_destination(sources: &SdPathBatch) -> Result<SdPath, String> {
	let first = sources
		.paths
		.first()
		.ok_or_else(|| "At least one source file must be specified".to_string())?;

	let (first_device_slug, first_parent) = physical_parent(first)?;

	for source in &sources.paths[1..] {
		let (device_slug, parent) = physical_parent(source)?;
		if device_slug != first_device_slug || parent != first_parent {
			return Err(
				"Duplicate currently requires all selected sources to share one parent directory"
					.to_string(),
			);
		}
	}

	Ok(SdPath::Physical {
		device_slug: first_device_slug,
		path: first_parent,
	})
}

fn physical_parent(path: &SdPath) -> Result<(String, std::path::PathBuf), String> {
	match path {
		SdPath::Physical { device_slug, path } => {
			let parent = path.parent().ok_or_else(|| {
				format!(
					"Cannot duplicate path without a parent directory: {}",
					path.display()
				)
			})?;

			Ok((device_slug.clone(), parent.to_path_buf()))
		}
		_ => Err("Duplicate currently supports only physical filesystem paths".to_string()),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn duplicate_destination_uses_shared_parent() {
		let sources = SdPathBatch::new(vec![
			SdPath::physical("device-a".to_string(), "/tmp/example/one.txt"),
			SdPath::physical("device-a".to_string(), "/tmp/example/two.txt"),
		]);

		let destination = duplicate_destination(&sources).expect("shared parent should resolve");

		assert_eq!(
			destination,
			SdPath::physical("device-a".to_string(), "/tmp/example")
		);
	}

	#[test]
	fn duplicate_destination_rejects_empty_sources() {
		let error =
			duplicate_destination(&SdPathBatch::default()).expect_err("empty source should fail");

		assert!(error.contains("At least one source"));
	}

	#[test]
	fn duplicate_destination_rejects_mixed_parents() {
		let sources = SdPathBatch::new(vec![
			SdPath::physical("device-a".to_string(), "/tmp/example/one.txt"),
			SdPath::physical("device-a".to_string(), "/tmp/other/two.txt"),
		]);

		let error = duplicate_destination(&sources).expect_err("mixed parents should fail");

		assert!(error.contains("share one parent"));
	}
}
