//! Output types for create file operations

use crate::domain::addressing::SdPath;

use serde::{Deserialize, Serialize};
use specta::Type;

/// Output from creating a file.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CreateFileOutput {
	/// Path to the created file.
	pub file_path: SdPath,
}

impl CreateFileOutput {
	/// Create output for a newly created file.
	pub fn new(file_path: SdPath) -> Self {
		Self { file_path }
	}
}
