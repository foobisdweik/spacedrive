//! File validation action handler

use super::{
	input::{
		FileOperationConflict, FileOperationPreflightInput, FileOperationPreflightIssue,
		FileOperationPreflightIssueKind, FileOperationPreflightOutput, FileSystemEntryKind,
		FileValidationActionOutput,
	},
	job::{ValidationJob, ValidationMode},
};
use crate::{
	context::CoreContext,
	domain::addressing::{SdPath, SdPathBatch},
	infra::{
		action::{error::ActionError, LibraryAction, ValidationResult},
		job::handle::JobReceipt,
	},
	ops::files::validation::input::FileOperationKind,
	ops::files::FileValidationInput,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidationAction {
	pub targets: SdPathBatch,
	pub verify_checksums: bool,
	pub deep_scan: bool,
	pub preflight: Option<FileOperationPreflightInput>,
}

impl ValidationAction {
	/// Create a new file validation action
	pub fn new(targets: SdPathBatch, verify_checksums: bool, deep_scan: bool) -> Self {
		Self {
			targets,
			verify_checksums,
			deep_scan,
			preflight: None,
		}
	}
}

// Implement the unified LibraryAction (replaces ActionHandler)
impl LibraryAction for ValidationAction {
	type Input = FileValidationInput;
	type Output = FileValidationActionOutput;

	fn from_input(input: Self::Input) -> Result<Self, String> {
		let paths = input
			.paths
			.into_iter()
			.map(|p| SdPath::local(p))
			.collect::<Vec<_>>();
		Ok(ValidationAction {
			targets: SdPathBatch { paths },
			verify_checksums: input.verify_checksums,
			deep_scan: input.deep_scan,
			preflight: input.preflight,
		})
	}

	async fn execute(
		self,
		library: std::sync::Arc<crate::library::Library>,
		context: Arc<CoreContext>,
	) -> Result<Self::Output, ActionError> {
		if let Some(preflight) = self.preflight {
			return build_preflight_output(preflight)
				.await
				.map(FileValidationActionOutput::Preflight);
		}

		// Create validation job
		let mode = if self.deep_scan {
			ValidationMode::Complete
		} else if self.verify_checksums {
			ValidationMode::Integrity
		} else {
			ValidationMode::Basic
		};

		let job = ValidationJob::new(self.targets, mode);

		// Dispatch job and return handle directly
		let job_handle = library
			.jobs()
			.dispatch(job)
			.await
			.map_err(ActionError::Job)?;

		Ok(FileValidationActionOutput::Job(JobReceipt::from(
			job_handle,
		)))
	}

	fn action_kind(&self) -> &'static str {
		"files.validation"
	}

	async fn validate(
		&self,
		_library: &std::sync::Arc<crate::library::Library>,
		_context: std::sync::Arc<crate::context::CoreContext>,
	) -> Result<ValidationResult, ActionError> {
		if let Some(preflight) = &self.preflight {
			if preflight.sources.paths.is_empty() {
				return Err(ActionError::Validation {
					field: "preflight.sources".to_string(),
					message: "At least one source file must be specified".to_string(),
				});
			}

			return Ok(ValidationResult::Success { metadata: None });
		}

		// Validate paths
		if self.targets.paths.is_empty() {
			return Err(ActionError::Validation {
				field: "paths".to_string(),
				message: "At least one path must be specified".to_string(),
			});
		}

		Ok(ValidationResult::Success { metadata: None })
	}
}

async fn build_preflight_output(
	preflight: FileOperationPreflightInput,
) -> Result<FileOperationPreflightOutput, ActionError> {
	let mut file_count = 0usize;
	let mut total_bytes = 0u64;
	let mut conflicts = Vec::new();
	let mut issues = Vec::new();

	let destination_path = match preflight.destination.as_local_path() {
		Some(path) => Some(path.to_path_buf()),
		None => {
			issues.push(FileOperationPreflightIssue {
				kind: FileOperationPreflightIssueKind::UnsupportedPath,
				path: Some(preflight.destination.clone()),
				message: "Only local destination preflight checks are currently supported"
					.to_string(),
			});
			None
		}
	};

	for source in &preflight.sources.paths {
		let Some(source_path) = source.as_local_path() else {
			issues.push(FileOperationPreflightIssue {
				kind: FileOperationPreflightIssueKind::UnsupportedPath,
				path: Some(source.clone()),
				message: "Only local source preflight checks are currently supported".to_string(),
			});
			continue;
		};

		let source_metadata = match tokio::fs::symlink_metadata(source_path).await {
			Ok(metadata) => metadata,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
				issues.push(FileOperationPreflightIssue {
					kind: FileOperationPreflightIssueKind::MissingSource,
					path: Some(source.clone()),
					message: format!("Source does not exist: {}", source_path.display()),
				});
				continue;
			}
			Err(e) => {
				issues.push(FileOperationPreflightIssue {
					kind: FileOperationPreflightIssueKind::IoError,
					path: Some(source.clone()),
					message: format!("Could not inspect source {}: {}", source_path.display(), e),
				});
				continue;
			}
		};

		let (source_file_count, source_total_bytes, mut source_issues) =
			summarize_path(source_path, &source_metadata).await?;
		issues.append(&mut source_issues);
		file_count += source_file_count;
		total_bytes += source_total_bytes;

		let Some(destination_path) = &destination_path else {
			continue;
		};

		let Some(actual_destination) = resolve_operation_destination(
			source_path,
			destination_path,
			preflight.sources.paths.len(),
		)
		.await?
		else {
			issues.push(FileOperationPreflightIssue {
				kind: FileOperationPreflightIssueKind::DestinationNotDirectory,
				path: Some(preflight.destination.clone()),
				message: format!(
					"Destination is not a directory: {}",
					destination_path.display()
				),
			});
			continue;
		};

		if let Some(parent) = actual_destination.parent() {
			match tokio::fs::metadata(parent).await {
				Ok(metadata) if metadata.is_dir() => {}
				Ok(_) => {
					issues.push(FileOperationPreflightIssue {
						kind: FileOperationPreflightIssueKind::DestinationNotDirectory,
						path: Some(SdPath::local(parent.to_path_buf())),
						message: format!(
							"Destination parent is not a directory: {}",
							parent.display()
						),
					});
					continue;
				}
				Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
					issues.push(FileOperationPreflightIssue {
						kind: FileOperationPreflightIssueKind::MissingDestinationParent,
						path: Some(SdPath::local(parent.to_path_buf())),
						message: format!("Destination parent does not exist: {}", parent.display()),
					});
					continue;
				}
				Err(e) => {
					issues.push(FileOperationPreflightIssue {
						kind: FileOperationPreflightIssueKind::IoError,
						path: Some(SdPath::local(parent.to_path_buf())),
						message: format!(
							"Could not inspect destination parent {}: {}",
							parent.display(),
							e
						),
					});
					continue;
				}
			}
		}

		if let Ok(existing_metadata) = tokio::fs::metadata(&actual_destination).await {
			conflicts.push(FileOperationConflict {
				source: source.clone(),
				destination: SdPath::local(actual_destination),
				existing_kind: entry_kind(&existing_metadata),
				existing_size: existing_metadata
					.is_file()
					.then_some(existing_metadata.len()),
			});
		}
	}

	let can_execute = issues.is_empty();
	let requires_confirmation = !conflicts.is_empty();

	Ok(FileOperationPreflightOutput {
		operation: preflight.operation,
		sources: preflight.sources,
		destination: preflight.destination,
		file_count,
		total_bytes,
		conflicts,
		issues,
		can_execute,
		requires_confirmation,
		supports_job_progress: true,
	})
}

async fn resolve_operation_destination(
	source_path: &Path,
	destination_path: &Path,
	source_count: usize,
) -> Result<Option<PathBuf>, ActionError> {
	match tokio::fs::metadata(destination_path).await {
		Ok(metadata) if metadata.is_dir() => Ok(source_path
			.file_name()
			.map(|name| destination_path.join(name))),
		Ok(_) if source_count == 1 => Ok(Some(destination_path.to_path_buf())),
		Ok(_) => Ok(None),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound && source_count == 1 => {
			Ok(Some(destination_path.to_path_buf()))
		}
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(source_path
			.file_name()
			.map(|name| destination_path.join(name))),
		Err(e) => Err(ActionError::Internal(format!(
			"Failed to inspect destination {}: {}",
			destination_path.display(),
			e
		))),
	}
}

async fn summarize_path(
	path: &Path,
	metadata: &std::fs::Metadata,
) -> Result<(usize, u64, Vec<FileOperationPreflightIssue>), ActionError> {
	if metadata.is_file() || metadata.file_type().is_symlink() {
		return Ok((1, metadata.len(), Vec::new()));
	}

	if !metadata.is_dir() {
		return Ok((1, 0, Vec::new()));
	}

	let mut count = 0usize;
	let mut size = 0u64;
	let mut issues = Vec::new();
	let mut stack = vec![path.to_path_buf()];

	while let Some(current) = stack.pop() {
		let metadata = match tokio::fs::symlink_metadata(&current).await {
			Ok(metadata) => metadata,
			Err(e) => {
				issues.push(FileOperationPreflightIssue {
					kind: FileOperationPreflightIssueKind::IoError,
					path: Some(SdPath::local(current.clone())),
					message: format!("Could not inspect {}: {}", current.display(), e),
				});
				continue;
			}
		};

		if metadata.is_file() || metadata.file_type().is_symlink() {
			count += 1;
			size += metadata.len();
		} else if metadata.is_dir() {
			let mut dir = match tokio::fs::read_dir(&current).await {
				Ok(dir) => dir,
				Err(e) => {
					issues.push(FileOperationPreflightIssue {
						kind: FileOperationPreflightIssueKind::IoError,
						path: Some(SdPath::local(current.clone())),
						message: format!("Could not read directory {}: {}", current.display(), e),
					});
					continue;
				}
			};

			loop {
				match dir.next_entry().await {
					Ok(Some(entry)) => stack.push(entry.path()),
					Ok(None) => break,
					Err(e) => {
						issues.push(FileOperationPreflightIssue {
							kind: FileOperationPreflightIssueKind::IoError,
							path: Some(SdPath::local(current.clone())),
							message: format!(
								"Could not read directory entry in {}: {}",
								current.display(),
								e
							),
						});
						break;
					}
				}
			}
		}
	}

	Ok((count, size, issues))
}

fn entry_kind(metadata: &std::fs::Metadata) -> FileSystemEntryKind {
	if metadata.is_file() {
		FileSystemEntryKind::File
	} else if metadata.is_dir() {
		FileSystemEntryKind::Directory
	} else {
		FileSystemEntryKind::Other
	}
}

// Register this action with the new registry
crate::register_library_action!(ValidationAction, "files.validation");

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::PathBuf;

	#[tokio::test]
	async fn test_preflight_detects_destination_conflict() {
		let temp_dir = tempfile::tempdir().expect("temp dir");
		let source = temp_dir.path().join("source.txt");
		let destination_dir = temp_dir.path().join("dest");
		let conflicting_destination = destination_dir.join("source.txt");
		tokio::fs::write(&source, b"source").await.expect("source");
		tokio::fs::create_dir(&destination_dir)
			.await
			.expect("dest dir");
		tokio::fs::write(&conflicting_destination, b"existing")
			.await
			.expect("conflict");

		let output = build_preflight_output(FileOperationPreflightInput {
			sources: SdPathBatch::new(vec![SdPath::local(source)]),
			destination: SdPath::local(destination_dir),
			operation: FileOperationKind::Copy,
		})
		.await
		.expect("preflight");

		assert!(output.can_execute);
		assert!(output.requires_confirmation);
		assert_eq!(output.file_count, 1);
		assert_eq!(output.total_bytes, 6);
		assert_eq!(output.conflicts.len(), 1);
		assert_eq!(output.conflicts[0].existing_kind, FileSystemEntryKind::File);
	}

	#[tokio::test]
	async fn test_preflight_reports_missing_source() {
		let temp_dir = tempfile::tempdir().expect("temp dir");
		let source = temp_dir.path().join("missing.txt");
		let destination_dir = temp_dir.path().join("dest");
		tokio::fs::create_dir(&destination_dir)
			.await
			.expect("dest dir");

		let output = build_preflight_output(FileOperationPreflightInput {
			sources: SdPathBatch::new(vec![SdPath::local(source)]),
			destination: SdPath::local(destination_dir),
			operation: FileOperationKind::Move,
		})
		.await
		.expect("preflight");

		assert!(!output.can_execute);
		assert!(!output.requires_confirmation);
		assert_eq!(output.issues.len(), 1);
		assert_eq!(
			output.issues[0].kind,
			FileOperationPreflightIssueKind::MissingSource
		);
	}

	#[tokio::test]
	async fn test_resolve_single_source_destination_file_path() {
		let temp_dir = tempfile::tempdir().expect("temp dir");
		let source = temp_dir.path().join("source.txt");
		let destination = temp_dir.path().join("renamed.txt");

		assert_eq!(
			resolve_operation_destination(&source, &destination, 1)
				.await
				.expect("resolve destination"),
			Some(destination)
		);
	}

	#[tokio::test]
	async fn test_resolve_accepts_single_source_existing_file_destination() {
		let temp_dir = tempfile::tempdir().expect("temp dir");
		let source = temp_dir.path().join("source.txt");
		let destination = temp_dir.path().join("destination.txt");
		tokio::fs::write(&destination, b"existing")
			.await
			.expect("destination");

		assert_eq!(
			resolve_operation_destination(&source, &destination, 1)
				.await
				.expect("resolve destination"),
			Some(destination)
		);
	}

	#[tokio::test]
	async fn test_preflight_reports_existing_file_destination_conflict() {
		let temp_dir = tempfile::tempdir().expect("temp dir");
		let source = temp_dir.path().join("source.txt");
		let destination = temp_dir.path().join("destination.txt");
		tokio::fs::write(&source, b"source").await.expect("source");
		tokio::fs::write(&destination, b"existing")
			.await
			.expect("destination");

		let output = build_preflight_output(FileOperationPreflightInput {
			sources: SdPathBatch::new(vec![SdPath::local(source)]),
			destination: SdPath::local(destination),
			operation: FileOperationKind::Copy,
		})
		.await
		.expect("preflight");

		assert!(output.can_execute);
		assert!(output.requires_confirmation);
		assert_eq!(output.conflicts.len(), 1);
		assert_eq!(output.conflicts[0].existing_kind, FileSystemEntryKind::File);
	}

	#[tokio::test]
	async fn test_resolve_missing_multi_source_destination_joins_source_name() {
		let temp_dir = tempfile::tempdir().expect("temp dir");
		let source = temp_dir.path().join("source.txt");
		let destination = temp_dir.path().join("missing-destination");

		assert_eq!(
			resolve_operation_destination(&source, &destination, 2)
				.await
				.expect("resolve destination"),
			Some(destination.join("source.txt"))
		);
	}
}
