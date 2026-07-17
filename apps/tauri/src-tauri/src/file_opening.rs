use file_opening::{FileOpener, OpenResult, OpenWithApp};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[cfg(target_os = "macos")]
use file_opening_macos::MacFileOpener as PlatformOpener;

#[cfg(target_os = "windows")]
use file_opening_windows::WindowsFileOpener as PlatformOpener;

#[cfg(target_os = "linux")]
use file_opening_linux::LinuxFileOpener as PlatformOpener;

pub struct FileOpeningService {
	// Arc (not Box) so command handlers can clone the opener into
	// spawn_blocking closures without borrowing managed state across await.
	opener: Arc<dyn FileOpener>,
}

impl FileOpeningService {
	pub fn new() -> Self {
		Self {
			opener: Arc::new(PlatformOpener),
		}
	}
}

/// Get applications that can open the given file paths
/// Returns intersection of compatible apps for multiple files
#[tauri::command]
pub async fn get_apps_for_paths(
	paths: Vec<PathBuf>,
	service: State<'_, FileOpeningService>,
) -> Result<Vec<OpenWithApp>, String> {
	if paths.is_empty() {
		return Ok(vec![]);
	}

	// Platform lookups (NSWorkspace, COM, GIO) are blocking FFI; keep them off
	// the async runtime so the UI stays responsive.
	let opener = service.opener.clone();
	tauri::async_runtime::spawn_blocking(move || opener.get_apps_for_files(&paths))
		.await
		.map_err(|e| e.to_string())?
}

/// Open file with system default application
#[tauri::command]
pub async fn open_path_default(
	path: PathBuf,
	service: State<'_, FileOpeningService>,
) -> Result<OpenResult, String> {
	let opener = service.opener.clone();
	tauri::async_runtime::spawn_blocking(move || opener.open_with_default(&path))
		.await
		.map_err(|e| e.to_string())?
}

/// Open file with specific application
#[tauri::command]
pub async fn open_path_with_app(
	path: PathBuf,
	app_id: String,
	service: State<'_, FileOpeningService>,
) -> Result<OpenResult, String> {
	let opener = service.opener.clone();
	tauri::async_runtime::spawn_blocking(move || opener.open_with_app(&path, &app_id))
		.await
		.map_err(|e| e.to_string())?
}

/// Open multiple files with specific application
#[tauri::command]
pub async fn open_paths_with_app(
	paths: Vec<PathBuf>,
	app_id: String,
	service: State<'_, FileOpeningService>,
) -> Result<Vec<OpenResult>, String> {
	let opener = service.opener.clone();
	tauri::async_runtime::spawn_blocking(move || opener.open_files_with_app(&paths, &app_id))
		.await
		.map_err(|e| e.to_string())?
}
