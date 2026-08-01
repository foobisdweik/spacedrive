//! Finished job clearing operation

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
	context::CoreContext,
	infra::action::{error::ActionResult, LibraryAction},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct JobClearInput {}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct JobClearOutput {
	pub cleared: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobClearAction;

impl LibraryAction for JobClearAction {
	type Input = JobClearInput;
	type Output = JobClearOutput;

	fn from_input(_input: Self::Input) -> Result<Self, String> {
		Ok(Self)
	}

	fn action_kind(&self) -> &'static str {
		"jobs.clear"
	}

	async fn execute(
		self,
		library: Arc<crate::library::Library>,
		_context: Arc<CoreContext>,
	) -> ActionResult<Self::Output> {
		let cleared = library.jobs().clear_finished_jobs().await?;
		Ok(JobClearOutput { cleared })
	}
}

crate::register_library_action!(JobClearAction, "jobs.clear");
