//! File duplicate action backed by the copy job.

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

use super::super::copy::action::FileCopyAction;
use super::input::FileDuplicateInput;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDuplicateAction {
	inner: FileCopyAction,
}

impl LibraryAction for FileDuplicateAction {
	type Output = JobReceipt;
	type Input = FileDuplicateInput;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		let copy_input = input.to_copy_input()?;
		let inner = FileCopyAction::from_input(copy_input)?;

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
		"files.duplicate"
	}
}

crate::register_library_action!(FileDuplicateAction, "files.duplicate");

#[cfg(test)]
mod tests {
	use crate::{
		domain::addressing::{SdPath, SdPathBatch},
		infra::action::LibraryAction,
		ops::files::copy::action::FileConflictResolution,
	};

	use super::*;

	#[test]
	fn from_input_duplicates_to_parent_with_auto_name_conflicts() {
		let temp_dir = tempfile::tempdir().expect("temp directory should be created");
		let source = temp_dir.path().join("one.txt");
		std::fs::write(&source, b"test").expect("source file should be created");

		let input = FileDuplicateInput {
			sources: SdPathBatch::new(vec![SdPath::local(source)]),
			..Default::default()
		};

		let action = FileDuplicateAction::from_input(input).expect("duplicate action should build");

		assert_eq!(
			action.inner.destination.as_local_path(),
			Some(temp_dir.path())
		);
		assert_eq!(
			action.inner.on_conflict,
			Some(FileConflictResolution::AutoModifyName)
		);
		assert_eq!(action.action_kind(), "files.duplicate");
	}
}
