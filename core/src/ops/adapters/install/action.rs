use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};

use super::{InstallAdapterInput, InstallAdapterOutput};
use crate::{
	context::CoreContext,
	infra::action::{error::ActionError, LibraryAction},
	library::Library,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallAdapterAction {
	input: InstallAdapterInput,
}

impl LibraryAction for InstallAdapterAction {
	type Input = InstallAdapterInput;
	type Output = InstallAdapterOutput;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		if input.directory.trim().is_empty() {
			return Err("Adapter directory cannot be empty".to_string());
		}

		Ok(Self { input })
	}

	async fn execute(
		self,
		library: Arc<Library>,
		_context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		if library.source_manager().is_none() {
			library.init_source_manager().await.map_err(|error| {
				ActionError::Internal(format!("Failed to initialize source manager: {error}"))
			})?;
		}

		let source_manager =
			Arc::clone(library.source_manager().ok_or_else(|| {
				ActionError::Internal("Source manager is unavailable".to_string())
			})?);
		let directory = PathBuf::from(self.input.directory);
		let adapter_id =
			tokio::task::spawn_blocking(move || source_manager.install_adapter(&directory))
				.await
				.map_err(|error| {
					ActionError::Internal(format!("Adapter installation task failed: {error}"))
				})?
				.map_err(ActionError::Internal)?;

		Ok(InstallAdapterOutput { adapter_id })
	}

	fn action_kind(&self) -> &'static str {
		"adapters.install"
	}
}

crate::register_library_action!(InstallAdapterAction, "adapters.install");
