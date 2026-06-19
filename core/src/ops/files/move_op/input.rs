//! Input types for explicit file move operations.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::domain::addressing::{SdPath, SdPathBatch};

use super::super::copy::{action::FileConflictResolution, input::CopyMethod};

/// Input for moving files or directories to a destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FileMoveInput {
	/// Source files or directories to move.
	pub sources: SdPathBatch,
	/// Destination path.
	pub destination: SdPath,
	/// Whether to overwrite existing destination files.
	pub overwrite: bool,
	/// Whether to verify checksums during cross-device moves.
	pub verify_checksum: bool,
	/// Whether to preserve file timestamps.
	pub preserve_timestamps: bool,
	/// Preferred copy/move method.
	pub copy_method: CopyMethod,
	/// How to handle file conflicts.
	pub on_conflict: Option<FileConflictResolution>,
}

impl FileMoveInput {
	pub fn validate(&self) -> Result<(), Vec<String>> {
		if self.sources.paths.is_empty() {
			Err(vec![
				"At least one source file must be specified".to_string()
			])
		} else {
			Ok(())
		}
	}
}

impl Default for FileMoveInput {
	fn default() -> Self {
		Self {
			sources: SdPathBatch { paths: Vec::new() },
			destination: SdPath::local(std::path::PathBuf::new()),
			overwrite: false,
			verify_checksum: false,
			preserve_timestamps: true,
			copy_method: CopyMethod::Auto,
			on_conflict: None,
		}
	}
}
