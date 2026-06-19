//! Input types for create file operations

use crate::domain::addressing::SdPath;

use serde::{Deserialize, Serialize};
use specta::Type;

/// Input for creating a new empty file.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CreateFileInput {
	/// Parent directory where the file will be created.
	pub parent: SdPath,
	/// Name for the new file.
	pub name: String,
}

impl CreateFileInput {
	/// Create a new file input.
	pub fn new(parent: SdPath, name: impl Into<String>) -> Self {
		Self {
			parent,
			name: name.into(),
		}
	}
}
