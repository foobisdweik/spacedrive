//! File validation input for external API

use crate::domain::addressing::{SdPath, SdPathBatch};

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

/// Input for file validation operations
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FileValidationInput {
	/// Paths to validate
	#[serde(default)]
	pub paths: Vec<PathBuf>,
	/// Whether to verify file checksums
	#[serde(default)]
	pub verify_checksums: bool,
	/// Whether to perform deep scanning
	#[serde(default)]
	pub deep_scan: bool,
	/// Optional Finder-style copy or move preflight request.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub preflight: Option<FileOperationPreflightInput>,
}

/// Finder-style file operation to preflight.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
pub enum FileOperationKind {
	Copy,
	Move,
}

/// Input for validating a copy or move before starting a job.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FileOperationPreflightInput {
	/// Source files or folders that will be copied or moved.
	pub sources: SdPathBatch,
	/// Destination directory or final destination path.
	pub destination: SdPath,
	/// Operation being prepared.
	pub operation: FileOperationKind,
}

/// Output from validating a copy or move before starting a job.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum FileValidationActionOutput {
	/// Legacy validation job was started.
	Job(crate::infra::job::handle::JobReceipt),
	/// Immediate preflight result for a file operation modal.
	Preflight(FileOperationPreflightOutput),
}

/// Structured result for Finder-style copy and move preflight checks.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct FileOperationPreflightOutput {
	pub operation: FileOperationKind,
	pub sources: SdPathBatch,
	pub destination: SdPath,
	pub file_count: usize,
	pub total_bytes: u64,
	pub conflicts: Vec<FileOperationConflict>,
	pub issues: Vec<FileOperationPreflightIssue>,
	pub can_execute: bool,
	pub requires_confirmation: bool,
	pub supports_job_progress: bool,
}

/// A destination conflict detected before starting a copy or move.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct FileOperationConflict {
	pub source: SdPath,
	pub destination: SdPath,
	pub existing_kind: FileSystemEntryKind,
	pub existing_size: Option<u64>,
}

/// File-system entry kind used by conflict preflight output.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
pub enum FileSystemEntryKind {
	File,
	Directory,
	Other,
}

/// Non-conflict issue that may prevent a file operation from starting.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct FileOperationPreflightIssue {
	pub kind: FileOperationPreflightIssueKind,
	pub path: Option<SdPath>,
	pub message: String,
}

/// Issue kind for file operation preflight checks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
pub enum FileOperationPreflightIssueKind {
	MissingSource,
	MissingDestinationParent,
	DestinationNotDirectory,
	UnsupportedPath,
	IoError,
}
