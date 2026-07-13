//! Extension-facing credential storage operations (PLUG-002).
//!
//! Exposes a `credentials.store` Wire action so sandboxed plugins can persist
//! their own cloud credentials through the generic `spacedrive_call` bridge.
//! The credential material is handed to [`CloudCredentialManager`], which
//! encrypts it (XChaCha20-Poly1305) under the library key before it touches the
//! database — the same path the built-in `volumes.add_cloud` action uses.

use crate::{
	context::CoreContext,
	crypto::cloud_credentials::{CloudCredential, CloudCredentialManager},
	infra::action::{error::ActionError, LibraryAction},
	volume::backend::CloudServiceType,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StoreCredentialInput {
	/// Key the credential is stored under (typically a volume fingerprint).
	pub volume_fingerprint: String,
	/// Cloud service this credential authenticates against.
	pub service: CloudServiceType,
	/// Credential material.
	pub credential: CredentialInput,
}

/// Plugin-facing credential payload. Mirrors the constructors on
/// [`CloudCredential`] with primitive fields so the internal crypto types don't
/// need to cross the Wire/specta boundary.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type")]
pub enum CredentialInput {
	/// Access key + secret (S3 and compatible services).
	AccessKey {
		access_key_id: String,
		secret_access_key: String,
		#[serde(default)]
		session_token: Option<String>,
	},
	/// OAuth tokens (Google Drive, Dropbox, OneDrive).
	OAuth {
		access_token: String,
		refresh_token: String,
		client_id: String,
		client_secret: String,
	},
	/// Simple API key (e.g. GCS service-account JSON).
	ApiKey { api_key: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StoreCredentialOutput {
	pub stored: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreCredentialAction {
	input: StoreCredentialInput,
}

impl StoreCredentialAction {
	pub fn new(input: StoreCredentialInput) -> Self {
		Self { input }
	}
}

impl LibraryAction for StoreCredentialAction {
	type Input = StoreCredentialInput;
	type Output = StoreCredentialOutput;

	fn from_input(input: StoreCredentialInput) -> Result<Self, String> {
		Ok(Self::new(input))
	}

	async fn execute(
		self,
		library: Arc<crate::library::Library>,
		context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		let StoreCredentialInput {
			volume_fingerprint,
			service,
			credential,
		} = self.input;

		if volume_fingerprint.trim().is_empty() {
			return Err(ActionError::InvalidInput(
				"volume_fingerprint must not be empty".to_string(),
			));
		}

		let credential = match credential {
			CredentialInput::AccessKey {
				access_key_id,
				secret_access_key,
				session_token,
			} => CloudCredential::new_access_key(
				service,
				access_key_id,
				secret_access_key,
				session_token,
			),
			CredentialInput::OAuth {
				access_token,
				refresh_token,
				client_id,
				client_secret,
			} => CloudCredential::new_oauth(
				service,
				access_token,
				refresh_token,
				client_id,
				client_secret,
				None,
			),
			CredentialInput::ApiKey { api_key } => {
				CloudCredential::new_api_key(service, api_key)
			}
		};

		let library_id = library.id();
		let manager = CloudCredentialManager::new(
			context.key_manager.clone(),
			library.db().clone(),
			library_id,
		);

		manager
			.store_credential(library_id, &volume_fingerprint, &credential)
			.await
			.map_err(|e| ActionError::Internal(format!("Failed to store credential: {}", e)))?;

		Ok(StoreCredentialOutput { stored: true })
	}

	fn action_kind(&self) -> &'static str {
		"credentials.store"
	}
}

crate::register_library_action!(StoreCredentialAction, "credentials.store");
