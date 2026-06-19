//! Explicit file move action backed by the copy job.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
	context::CoreContext,
	infra::{
		action::{error::ActionError, LibraryAction, ValidationResult},
		job::handle::JobReceipt,
	},
	library::Library,
};

use super::super::copy::{action::FileCopyAction, input::FileCopyInput, job::MoveMode};
use super::input::FileMoveInput;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMoveAction {
	inner: FileCopyAction,
}

impl FileMoveAction {
	fn copy_input(input: FileMoveInput) -> Result<FileCopyInput, String> {
		input.validate().map_err(|errors| errors.join("; "))?;

		Ok(FileCopyInput {
			sources: input.sources,
			destination: input.destination,
			overwrite: input.overwrite,
			verify_checksum: input.verify_checksum,
			preserve_timestamps: input.preserve_timestamps,
			move_files: true,
			copy_method: input.copy_method,
			on_conflict: input.on_conflict,
		})
	}
}

impl LibraryAction for FileMoveAction {
	type Output = JobReceipt;
	type Input = FileMoveInput;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		let copy_input = Self::copy_input(input)?;
		let mut inner = FileCopyAction::from_input(copy_input)?;
		inner.options.move_mode = Some(MoveMode::Move);

		Ok(Self { inner })
	}

	async fn validate(
		&self,
		library: &Arc<Library>,
		context: Arc<CoreContext>,
	) -> Result<ValidationResult, ActionError> {
		self.inner.validate(library, context).await
	}

	fn resolve_confirmation(&mut self, choice_index: usize) -> Result<(), ActionError> {
		self.inner.resolve_confirmation(choice_index)
	}

	async fn execute(
		self,
		library: Arc<Library>,
		_context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let action_kind = self.action_kind();
		let job_handle = self
			.inner
			.execute_with_action_kind(library, action_kind)
			.await?;

		Ok(job_handle.into())
	}

	fn action_kind(&self) -> &'static str {
		"files.move"
	}
}

crate::register_library_action!(FileMoveAction, "files.move");

#[cfg(test)]
mod tests {
	use crate::{
		domain::addressing::{SdPath, SdPathBatch},
		infra::action::LibraryAction,
	};

	use super::*;

	#[test]
	fn from_input_forces_move_options() {
		let temp_dir = tempfile::tempdir().expect("temp directory should be created");
		let source = temp_dir.path().join("source.txt");
		let destination = temp_dir.path().join("destination.txt");
		std::fs::write(&source, b"test").expect("source file should be created");

		let input = FileMoveInput {
			sources: SdPathBatch::new(vec![SdPath::local(source)]),
			destination: SdPath::local(destination),
			..Default::default()
		};

		let action = FileMoveAction::from_input(input).expect("move action should build");

		assert!(action.inner.options.delete_after_copy);
		assert_eq!(action.inner.options.move_mode, Some(MoveMode::Move));
		assert_eq!(action.action_kind(), "files.move");
	}

	#[test]
	fn from_input_rejects_empty_sources() {
		let error = FileMoveAction::from_input(FileMoveInput::default())
			.expect_err("empty sources should be rejected");

		assert!(error.contains("At least one source file"));
	}
}
