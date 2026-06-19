//! Create file action handler

use super::{input::CreateFileInput, output::CreateFileOutput};
use crate::{
	context::CoreContext,
	domain::addressing::SdPath,
	infra::action::{error::ActionError, LibraryAction, ValidationResult},
	ops::files::rename::validation::validate_filename,
};

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tracing::debug;

/// Action for creating a new empty file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFileAction {
	/// Parent directory where the file will be created.
	pub parent: SdPath,
	/// Name for the new file.
	pub name: String,
}

impl CreateFileAction {
	/// Create a new file action.
	pub fn new(parent: SdPath, name: impl Into<String>) -> Self {
		Self {
			parent,
			name: name.into(),
		}
	}
}

impl LibraryAction for CreateFileAction {
	type Input = CreateFileInput;
	type Output = CreateFileOutput;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		Ok(Self {
			parent: input.parent,
			name: input.name,
		})
	}

	async fn validate(
		&self,
		_library: &Arc<crate::library::Library>,
		_context: Arc<CoreContext>,
	) -> Result<ValidationResult, ActionError> {
		validate_filename(&self.name).map_err(|e| ActionError::Validation {
			field: "name".to_string(),
			message: e.to_string(),
		})?;

		match &self.parent {
			SdPath::Physical { .. } | SdPath::Cloud { .. } => {}
			SdPath::Content { .. } => {
				return Err(ActionError::Validation {
					field: "parent".to_string(),
					message: "Cannot create files in content-addressed storage".to_string(),
				});
			}
			SdPath::Sidecar { .. } => {
				return Err(ActionError::Validation {
					field: "parent".to_string(),
					message: "Cannot create files in sidecar storage".to_string(),
				});
			}
		}

		Ok(ValidationResult::Success { metadata: None })
	}

	async fn execute(
		self,
		_library: Arc<crate::library::Library>,
		_context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let file_path = self.parent.join(&self.name);

		debug!(
			"Creating file: {} in parent: {}",
			self.name,
			self.parent.display()
		);

		match &file_path {
			SdPath::Physical { path, .. } => {
				create_new_empty_file(path).await?;
			}
			SdPath::Cloud { .. } => {
				return Err(ActionError::Internal(
					"Cloud file creation not yet implemented".to_string(),
				));
			}
			_ => {
				return Err(ActionError::Internal(
					"Unexpected path type after validation".to_string(),
				));
			}
		}

		Ok(CreateFileOutput::new(file_path))
	}

	fn action_kind(&self) -> &'static str {
		"files.createFile"
	}
}

async fn create_new_empty_file(path: &std::path::Path) -> Result<(), ActionError> {
	if let Some(parent) = path.parent() {
		if !tokio::fs::try_exists(parent).await.unwrap_or(false) {
			return Err(ActionError::Validation {
				field: "parent".to_string(),
				message: format!("Parent directory does not exist: {}", parent.display()),
			});
		}
	}

	OpenOptions::new()
		.write(true)
		.create_new(true)
		.open(path)
		.await
		.map_err(|e| ActionError::Internal(format!("Failed to create file: {}", e)))?;

	Ok(())
}

crate::register_library_action!(CreateFileAction, "files.createFile");

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::PathBuf;

	#[test]
	fn test_action_creation() {
		let parent = SdPath::local(PathBuf::from("/test"));
		let action = CreateFileAction::new(parent, "new_file.txt");
		assert_eq!(action.name, "new_file.txt");
	}

	#[tokio::test]
	async fn test_create_new_empty_file() {
		let temp_dir = tempfile::tempdir().expect("temp dir");
		let file_path = temp_dir.path().join("new_file.txt");

		create_new_empty_file(&file_path)
			.await
			.expect("create file");

		let metadata = tokio::fs::metadata(&file_path).await.expect("metadata");
		assert!(metadata.is_file());
		assert_eq!(metadata.len(), 0);
	}

	#[tokio::test]
	async fn test_create_new_empty_file_rejects_existing_file() {
		let temp_dir = tempfile::tempdir().expect("temp dir");
		let file_path = temp_dir.path().join("existing.txt");
		tokio::fs::write(&file_path, b"existing")
			.await
			.expect("seed file");

		let err = create_new_empty_file(&file_path)
			.await
			.expect_err("existing file should fail");

		assert!(matches!(err, ActionError::Internal(_)));
	}
}
