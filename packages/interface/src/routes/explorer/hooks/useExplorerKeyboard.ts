import type {
	DirectoryListingOutput,
	DirectorySortBy,
	SdPath
} from '@sd/ts-client';
import {useEffect} from 'react';
import {useFileOperationDialog} from '../../../components/modals/FileOperationModal';
import {
	useLibraryMutation,
	useNormalizedQuery
} from '../../../contexts/SpacedriveContext';
import {useClipboard} from '../../../hooks/useClipboard';
import {useKeybind} from '../../../hooks/useKeybind';
import {useKeybindScope} from '../../../hooks/useKeybindScope';
import {isInputFocused} from '../../../util/keybinds/platform';
import {useExplorer} from '../context';
import {useSelection} from '../SelectionContext';
import {useDeleteFiles} from './useDeleteFiles';
import {useTypeaheadSearch} from './useTypeaheadSearch';

export function useExplorerKeyboard() {
	const {
		currentPath,
		sortBy,
		navigateToPath,
		viewMode,
		viewSettings,
		sidebarVisible,
		inspectorVisible,
		openQuickPreview,
		tagModeActive,
		setTagModeActive
	} = useExplorer();
	const {
		selectedFiles,
		selectAll,
		clearSelection,
		focusedIndex,
		setFocusedIndex,
		setSelectedFiles,
		startRename,
		isRenaming
	} = useSelection();
	const clipboard = useClipboard();
	const openFileOperation = useFileOperationDialog();
	const {deleteFiles, isPending: isDeleting} = useDeleteFiles();
	const duplicateFiles = useLibraryMutation('files.duplicate');

	// Activate explorer keybind scope when this hook is active
	useKeybindScope('explorer');

	// Query files for keyboard operations
	const directoryQuery = useNormalizedQuery({
		query: 'files.directory_listing',
		input: currentPath
			? {
					path: currentPath,
					limit: null,
					include_hidden: false,
					sort_by: sortBy as DirectorySortBy
				}
			: null!,
		resourceType: 'file',
		enabled: !!currentPath,
		pathScope: currentPath ?? undefined
	});

	const files =
		(directoryQuery.data as DirectoryListingOutput | undefined)?.files ||
		[];
	const selectedFilePaths = selectedFiles.map((f) => f.sd_path);
	const canDuplicateSelection = canDuplicatePaths(selectedFilePaths);

	// Typeahead search (disabled for column view - it handles its own)
	const typeahead = useTypeaheadSearch({
		files,
		onMatch: (file, index) => {
			setFocusedIndex(index);
			setSelectedFiles([file]);
		},
		enabled: viewMode !== 'column'
	});

	// Copy: Store selected files in clipboard
	useKeybind(
		'explorer.copy',
		() => {
			if (isRenaming) return;
			if (selectedFiles.length === 0) return;
			const sdPaths = selectedFiles.map((f) => f.sd_path);
			clipboard.copyFiles(sdPaths, currentPath);
		},
		{enabled: selectedFiles.length > 0 && !isRenaming}
	);

	// Cut: Store selected files in clipboard with cut operation
	useKeybind(
		'explorer.cut',
		() => {
			if (isRenaming) return;
			if (selectedFiles.length === 0) return;
			const sdPaths = selectedFiles.map((f) => f.sd_path);
			clipboard.cutFiles(sdPaths, currentPath);
		},
		{enabled: selectedFiles.length > 0 && !isRenaming}
	);

	useKeybind(
		'explorer.duplicate',
		() => {
			if (isRenaming) return;
			if (selectedFiles.length === 0) return;
			duplicateFiles.mutate({
				sources: {paths: selectedFilePaths},
				verify_checksum: false,
				preserve_timestamps: true,
				copy_method: 'Auto'
			});
		},
		{
			enabled:
				selectedFiles.length > 0 && !isRenaming && canDuplicateSelection
		}
	);

	// Paste: Open file operation modal with clipboard contents
	useKeybind(
		'explorer.paste',
		() => {
			if (isRenaming) return;
			if (!clipboard.hasClipboard() || !currentPath) return;

			const operation = clipboard.operation === 'cut' ? 'move' : 'copy';

			console.groupCollapsed(
				`[Clipboard] Pasting ${clipboard.files.length} file${clipboard.files.length === 1 ? '' : 's'} (${operation})`
			);
			console.log('Operation:', operation);
			console.log('Destination:', currentPath);
			console.log('Source files (SdPath objects):');
			clipboard.files.forEach((file, index) => {
				console.log(`  [${index}]:`, JSON.stringify(file, null, 2));
			});
			console.groupEnd();

			openFileOperation({
				operation,
				sources: clipboard.files,
				destination: currentPath,
				onComplete: (completedOperation) => {
					if (completedOperation === 'move') {
						console.log(
							'[Clipboard] Operation completed, clearing clipboard'
						);
						clipboard.clearClipboard();
					} else {
						console.log('[Clipboard] Copy operation completed');
					}
				}
			});
		},
		{enabled: clipboard.hasClipboard() && !!currentPath && !isRenaming}
	);

	// Rename: Enter (or F2) triggers rename mode for any selected file or directory
	const triggerRename = () => {
		if (selectedFiles.length === 1 && !isRenaming) {
			startRename(selectedFiles[0].id);
		}
	};
	useKeybind('explorer.renameFile', triggerRename, {
		enabled: selectedFiles.length === 1 && !isRenaming
	});
	useKeybind('explorer.renameFileAlt', triggerRename, {
		enabled: selectedFiles.length === 1 && !isRenaming
	});

	// Tag mode: T key enters tag assignment mode
	useKeybind(
		'explorer.enterTagMode',
		() => {
			setTagModeActive(true);
		},
		{enabled: !tagModeActive}
	);

	// Quick Preview: Spacebar opens quick preview
	useKeybind(
		'explorer.toggleQuickPreview',
		() => {
			if (selectedFiles.length === 1) {
				openQuickPreview(selectedFiles[0].id);
			}
		},
		{enabled: selectedFiles.length === 1}
	);

	// Delete: Move to trash
	useKeybind(
		'explorer.delete',
		async () => {
			if (isRenaming) return;
			const ok = await deleteFiles(selectedFiles, false);
			if (ok) clearSelection();
		},
		{enabled: selectedFiles.length > 0 && !isDeleting && !isRenaming}
	);

	// Permanent Delete: Shift+Delete / Cmd+Alt+Backspace
	useKeybind(
		'explorer.permanentDelete',
		async () => {
			if (isRenaming) return;
			const ok = await deleteFiles(selectedFiles, true);
			if (ok) clearSelection();
		},
		{enabled: selectedFiles.length > 0 && !isDeleting && !isRenaming}
	);

	useEffect(() => {
		const handleKeyDown = async (e: KeyboardEvent) => {
			// Skip all keyboard shortcuts if renaming or typing in an input
			if (isRenaming || isInputFocused()) return;

			// Arrow keys: Navigation
			if (
				['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(
					e.key
				)
			) {
				// Skip views that handle their own keyboard navigation
				if (
					viewMode === 'column' ||
					viewMode === 'media' ||
					viewMode === 'grid'
				) {
					return;
				}

				e.preventDefault();

				if (files.length === 0) return;

				let newIndex = focusedIndex;

				if (viewMode === 'list') {
					// List view: only up/down
					if (e.key === 'ArrowUp')
						newIndex = Math.max(0, focusedIndex - 1);
					if (e.key === 'ArrowDown')
						newIndex = Math.min(files.length - 1, focusedIndex + 1);
				}

				if (newIndex !== focusedIndex) {
					setFocusedIndex(newIndex);
					setSelectedFiles([files[newIndex]]);
				}
				return;
			}

			// Cmd/Ctrl+A: Select all
			if ((e.metaKey || e.ctrlKey) && e.key === 'a') {
				e.preventDefault();
				selectAll(files);
				return;
			}

			// Escape: Clear selection
			if (e.code === 'Escape' && selectedFiles.length > 0) {
				clearSelection();
			}

			// Typeahead search (handled by hook, disabled for column view)
			typeahead.handleKey(e);
		};

		window.addEventListener('keydown', handleKeyDown);
		return () => {
			window.removeEventListener('keydown', handleKeyDown);
			typeahead.cleanup();
		};
	}, [
		selectedFiles,
		files,
		focusedIndex,
		viewMode,
		viewSettings,
		sidebarVisible,
		inspectorVisible,
		selectAll,
		clearSelection,
		navigateToPath,
		setFocusedIndex,
		setSelectedFiles,
		openQuickPreview,
		isRenaming,
		typeahead
	]);
}

function canDuplicatePaths(paths: SdPath[]) {
	const firstParent = duplicateParentKey(paths[0]);
	return (
		!!firstParent &&
		paths.every((path) => duplicateParentKey(path) === firstParent)
	);
}

function duplicateParentKey(path: SdPath | undefined) {
	if (!path || !('Physical' in path)) return null;

	const normalizedPath = path.Physical.path
		.replace(/\\/g, '/')
		.replace(/\/+$/, '');
	if (
		normalizedPath === '' ||
		normalizedPath === '/' ||
		normalizedPath.endsWith(':')
	) {
		return null;
	}

	const separatorIndex = normalizedPath.lastIndexOf('/');
	if (separatorIndex < 0) return null;

	return `${path.Physical.device_slug}:${normalizedPath.slice(0, separatorIndex) || '/'}`;
}
