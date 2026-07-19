import type {
	DirectoryListingOutput,
	File,
	FileSearchOutput,
	SdPath
} from '@sd/ts-client';
import {useQueryClient} from '@tanstack/react-query';
import {
	createContext,
	useCallback,
	useContext,
	useEffect,
	useMemo,
	useState,
	type ReactNode
} from 'react';
import {toast} from '@spacedrive/primitives';
import {useTabManager} from '../../components/TabManager';
import {usePlatform} from '../../contexts/PlatformContext';
import {useLibraryMutation} from '../../contexts/SpacedriveContext';
import {useClipboard} from '../../hooks/useClipboard';

interface SelectionContextValue {
	selectedFiles: File[];
	selectedFileIds: Set<string>;
	isSelected: (fileId: string) => boolean;
	setSelectedFiles: (files: File[]) => void;
	selectFile: (
		file: File,
		files: File[],
		multi?: boolean,
		range?: boolean
	) => void;
	clearSelection: () => void;
	selectAll: (files: File[]) => void;
	focusedIndex: number;
	setFocusedIndex: (index: number) => void;
	moveFocus: (
		direction: 'up' | 'down' | 'left' | 'right',
		files: File[]
	) => void;
	// Rename state
	renamingFileId: string | null;
	startRename: (fileId: string) => void;
	cancelRename: () => void;
	saveRename: (newName: string) => Promise<void>;
	isRenaming: boolean;
	// Restore selection from available files (called by views when files load)
	restoreSelectionFromFiles: (files: File[]) => void;
}

const SelectionContext = createContext<SelectionContextValue | null>(null);

interface SelectionProviderProps {
	children: ReactNode;
	isActiveTab?: boolean;
}

function sdPathsEqual(
	left: SdPath | null | undefined,
	right: SdPath | null | undefined
): boolean {
	if (!left || !right) return left === right;

	if ('Physical' in left || 'Physical' in right) {
		return (
			'Physical' in left &&
			'Physical' in right &&
			left.Physical.device_slug === right.Physical.device_slug &&
			left.Physical.path === right.Physical.path
		);
	}

	if ('Cloud' in left || 'Cloud' in right) {
		return (
			'Cloud' in left &&
			'Cloud' in right &&
			left.Cloud.service === right.Cloud.service &&
			left.Cloud.identifier === right.Cloud.identifier &&
			left.Cloud.path === right.Cloud.path
		);
	}

	if ('Content' in left || 'Content' in right) {
		return (
			'Content' in left &&
			'Content' in right &&
			left.Content.content_id === right.Content.content_id
		);
	}

	return (
		'Sidecar' in left &&
		'Sidecar' in right &&
		left.Sidecar.content_id === right.Sidecar.content_id &&
		left.Sidecar.kind === right.Sidecar.kind &&
		left.Sidecar.variant === right.Sidecar.variant &&
		left.Sidecar.format === right.Sidecar.format
	);
}

// A stable string key for a location-addressable path, used to re-link a selected
// file to its renamed/moved counterpart (which arrives under a new id). Content and
// Sidecar paths are not location-stable across a rename, so they return undefined.
function sdPathKey(path: SdPath | null | undefined): string | undefined {
	if (!path) return undefined;
	if ('Physical' in path) {
		return `p:${path.Physical.device_slug}:${path.Physical.path}`;
	}
	if ('Cloud' in path) {
		return `c:${path.Cloud.service}:${path.Cloud.identifier}:${path.Cloud.path}`;
	}
	return undefined;
}

export function SelectionProvider({
	children,
	isActiveTab = true
}: SelectionProviderProps) {
	const platform = usePlatform();
	const clipboard = useClipboard();
	const tabManager = useTabManager();
	const queryClient = useQueryClient();
	const {activeTabId, tabs, getSelectionIds, updateSelectionIds} = tabManager;
	const renameFile = useLibraryMutation('files.rename');

	const activeTab = tabs.find((t) => t.id === activeTabId);
	const currentPath = activeTab?.savedPath ?? '/';

	// Local state for File objects (not serializable, can't be stored in TabManager)
	const [selectedFiles, setSelectedFilesInternal] = useState<File[]>([]);
	const [focusedIndex, setFocusedIndex] = useState(-1);
	const [, setLastSelectedIndex] = useState(-1);
	const [renamingFileId, setRenamingFileId] = useState<string | null>(null);

	// Track the stored IDs for the active tab and path (separate from File objects)
	const storedIds = getSelectionIds(activeTabId, currentPath);

	// Clear selection when activeTabId or path changes (we'll restore it when files load)
	useEffect(() => {
		setSelectedFilesInternal([]);
		setFocusedIndex(-1);
		setLastSelectedIndex(-1);
	}, [activeTabId, currentPath]);

	// Wrapper for setSelectedFiles that syncs to TabManager
	// Supports both direct values and updater functions
	const setSelectedFiles = useCallback(
		(filesOrUpdater: File[] | ((prev: File[]) => File[])) => {
			setSelectedFilesInternal((prev) => {
				const nextFiles =
					typeof filesOrUpdater === 'function'
						? filesOrUpdater(prev)
						: filesOrUpdater;

				// Sync to TabManager
				updateSelectionIds(
					activeTabId,
					currentPath,
					nextFiles.map((f) => f.id)
				);

				return nextFiles;
			});
		},
		[activeTabId, currentPath, updateSelectionIds]
	);

	// Sync selected file IDs to platform (for cross-window state sharing)
	// Only sync for the active tab to avoid conflicts
	useEffect(() => {
		if (!isActiveTab) return;

		const fileIds = selectedFiles.map((f) => f.id);

		if (platform.setSelectedFileIds) {
			platform.setSelectedFileIds(fileIds).catch((err) => {
				console.error(
					'Failed to sync selected files to platform:',
					err
				);
			});
		}
	}, [selectedFiles, platform, isActiveTab]);

	// Update native menu items based on selection and clipboard state
	// Only update for active tab
	useEffect(() => {
		if (!isActiveTab) return;

		const hasSelection = selectedFiles.length > 0;
		const isSingleSelection = selectedFiles.length === 1;

		platform.updateMenuItems?.([
			// NOTE: copy/cut/paste are always enabled to support text input operations
			// They intelligently route to file ops or native clipboard based on focus
			{id: 'duplicate', enabled: hasSelection},
			{id: 'rename', enabled: isSingleSelection},
			{id: 'delete', enabled: hasSelection}
		]);
	}, [selectedFiles, clipboard, platform, isActiveTab]);

	const clearSelection = useCallback(() => {
		setSelectedFiles([]);
		setFocusedIndex(-1);
		setLastSelectedIndex(-1);
	}, [setSelectedFiles]);

	const selectAll = useCallback(
		(files: File[]) => {
			setSelectedFiles([...files]);
			setLastSelectedIndex(files.length - 1);
		},
		[setSelectedFiles]
	);

	const selectFile = useCallback(
		(file: File, files: File[], multi = false, range = false) => {
			const fileIndex = files.findIndex((f) => f.id === file.id);

			if (range) {
				setLastSelectedIndex((prevLastIndex) => {
					if (prevLastIndex !== -1) {
						const start = Math.min(prevLastIndex, fileIndex);
						const end = Math.max(prevLastIndex, fileIndex);
						const rangeFiles = files.slice(start, end + 1);

						setSelectedFiles((prev) => {
							// If there's already a multi-file selection, add the range (Finder behavior)
							if (prev.length > 1) {
								// Create a map for O(1) lookup
								const existingIds = new Set(
									prev.map((f) => f.id)
								);
								const combined = [...prev];

								// Add new range files that aren't already selected
								for (const rangeFile of rangeFiles) {
									if (!existingIds.has(rangeFile.id)) {
										combined.push(rangeFile);
									}
								}

								return combined;
							} else {
								// Single file or empty selection, replace with range
								return rangeFiles;
							}
						});
					}
					return fileIndex; // Update anchor to clicked file for next range
				});
				setFocusedIndex(fileIndex);
			} else if (multi) {
				setSelectedFiles((prev) => {
					const isSelected = prev.some((f) => f.id === file.id);
					if (isSelected) {
						return prev.filter((f) => f.id !== file.id);
					} else {
						return [...prev, file];
					}
				});
				setFocusedIndex(fileIndex);
				setLastSelectedIndex(fileIndex);
			} else {
				setSelectedFiles([file]);
				setFocusedIndex(fileIndex);
				setLastSelectedIndex(fileIndex);
			}
		},
		[setSelectedFiles]
	);

	const moveFocus = useCallback(
		(direction: 'up' | 'down' | 'left' | 'right', files: File[]) => {
			if (files.length === 0) return;

			setFocusedIndex((currentFocusedIndex) => {
				let newIndex = currentFocusedIndex;

				if (direction === 'up')
					newIndex = Math.max(0, currentFocusedIndex - 1);
				if (direction === 'down')
					newIndex = Math.min(
						files.length - 1,
						currentFocusedIndex + 1
					);
				if (direction === 'left')
					newIndex = Math.max(0, currentFocusedIndex - 1);
				if (direction === 'right')
					newIndex = Math.min(
						files.length - 1,
						currentFocusedIndex + 1
					);

				if (newIndex !== currentFocusedIndex) {
					setSelectedFiles([files[newIndex]]);
					setLastSelectedIndex(newIndex);
				}

				return newIndex;
			});
		},
		[setSelectedFiles]
	);

	// Rename functions
	const startRename = useCallback((fileId: string) => {
		setRenamingFileId(fileId);
	}, []);

	const cancelRename = useCallback(() => {
		setRenamingFileId(null);
	}, []);

	const saveRename = useCallback(
		async (newName: string) => {
			if (!renamingFileId) return;

			const file = selectedFiles.find((f) => f.id === renamingFileId);
			if (!file) {
				setRenamingFileId(null);
				return;
			}

			// Don't submit if name is empty or unchanged
			const currentFullName = file.extension
				? `${file.name}.${file.extension}`
				: file.name;
			if (!newName.trim() || newName === currentFullName) {
				setRenamingFileId(null);
				return;
			}

			try {
				const splitFileName = (fileName: string) => {
					const lastDot = fileName.lastIndexOf('.');
					if (lastDot > 0 && lastDot < fileName.length - 1) {
						return {
							name: fileName.slice(0, lastDot),
							extension: fileName.slice(lastDot + 1)
						};
					}

					return {name: fileName, extension: null};
				};

				const renamePath = (path: string) => {
					const separatorIndex = path.lastIndexOf('/');
					if (separatorIndex === -1) return newName;

					return `${path.slice(0, separatorIndex + 1)}${newName}`;
				};

				const renameSdPath = (sdPath: SdPath): SdPath => {
					if ('Physical' in sdPath) {
						return {
							Physical: {
								...sdPath.Physical,
								path: renamePath(sdPath.Physical.path)
							}
						};
					}

					if ('Cloud' in sdPath) {
						return {
							Cloud: {
								...sdPath.Cloud,
								path: renamePath(sdPath.Cloud.path)
							}
						};
					}

					return sdPath;
				};

				const updateRenamedFile = (candidate: File): File => {
					if (candidate.id !== file.id) return candidate;

					if (candidate.kind === 'File') {
						return {
							...candidate,
							sd_path: renameSdPath(candidate.sd_path),
							...splitFileName(newName)
						};
					}

					return {
						...candidate,
						sd_path: renameSdPath(candidate.sd_path),
						name: newName,
						extension: null
					};
				};

				// Optimistically update the visible name while invalidation reloads authoritative metadata.
				queryClient.setQueriesData<DirectoryListingOutput>(
					{
						predicate: (query) =>
							Array.isArray(query.queryKey) &&
							typeof query.queryKey[0] === 'string' &&
							query.queryKey[0] ===
								'query:files.directory_listing'
					},
					(oldData) => {
						if (!oldData) return oldData;
						if (!Array.isArray(oldData.files)) return oldData;

						return {
							...oldData,
							files: oldData.files.map(updateRenamedFile)
						};
					}
				);
				queryClient.setQueriesData<FileSearchOutput>(
					{
						predicate: (query) =>
							Array.isArray(query.queryKey) &&
							typeof query.queryKey[0] === 'string' &&
							query.queryKey[0] === 'query:search.files'
					},
					(oldData) => {
						if (!oldData) return oldData;
						if (!Array.isArray(oldData.files)) return oldData;

						return {
							...oldData,
							files: oldData.files.map(updateRenamedFile)
						};
					}
				);

				await renameFile.mutateAsync({
					target: file.sd_path,
					new_name: newName
				});
				setRenamingFileId(null);

				// Keep the selection (and therefore the Inspector) pointed at the
				// renamed object by applying the same transform to the selected File
				// snapshots. We intentionally do NOT invalidate here: rename runs as a
				// background job that resolves at dispatch, so an immediate refetch
				// returns the pre-rename listing and clobbers the optimistic edit. The
				// authoritative refresh happens on job completion (useJobsDesktop.ts),
				// and cache reconciliation collapses the old/new rows by path.
				setSelectedFilesInternal((prev) => prev.map(updateRenamedFile));
			} catch (error) {
				// Rename failed (e.g. a name collision). Revert the optimistic edit by
				// refetching authoritative state and surface a clear error instead of
				// silently leaving the file unchanged. Stay in edit mode so the user
				// can correct the name.
				console.error('Rename failed:', error);
				await queryClient.invalidateQueries({
					predicate: (query) =>
						Array.isArray(query.queryKey) &&
						typeof query.queryKey[0] === 'string' &&
						(query.queryKey[0] ===
							'query:files.directory_listing' ||
							query.queryKey[0] === 'query:search.files')
				});
				toast.error({
					title: 'Rename failed',
					body: `Couldn't rename to "${newName}". ${String(error).replace(/^Error:\s*/, '')}`
				});
				throw error;
			}
		},
		[renamingFileId, selectedFiles, renameFile, queryClient]
	);

	// Cancel rename when selection changes
	useEffect(() => {
		if (
			renamingFileId &&
			!selectedFiles.some((f) => f.id === renamingFileId)
		) {
			setRenamingFileId(null);
		}
	}, [selectedFiles, renamingFileId]);

	// Use stored IDs for selection checking (allows highlighting before File objects are restored)
	const selectedFileIds = useMemo(() => new Set(storedIds), [storedIds]);

	// Stable function for checking if a file is selected
	const isSelected = useCallback(
		(fileId: string) => selectedFileIds.has(fileId),
		[selectedFileIds]
	);

	// Restore File objects for selected IDs when files become available
	const restoreSelectionFromFiles = useCallback(
		(files: File[]) => {
			if (storedIds.length === 0) return;

			const fileById = new Map(files.map((f) => [f.id, f]));
			const fileByPath = new Map<string, File>();
			for (const f of files) {
				const key = sdPathKey(f.sd_path);
				if (key) fileByPath.set(key, f);
			}

			setSelectedFilesInternal((prev) => {
				const prevById = new Map(prev.map((f) => [f.id, f]));
				const resolved: File[] = [];
				const seen = new Set<string>();

				for (const id of storedIds) {
					// Prefer an exact id match from the authoritative listing.
					let file = fileById.get(id);
					// If the id vanished, the object may have been renamed/moved (new
					// id, same path). Re-link via the previously-selected object's path
					// so the Inspector follows the renamed file instead of stranding on
					// a stale or deleted object.
					if (!file) {
						const previous = prevById.get(id);
						const prevKey = previous
							? sdPathKey(previous.sd_path)
							: undefined;
						if (prevKey) file = fileByPath.get(prevKey);
					}
					if (file && !seen.has(file.id)) {
						seen.add(file.id);
						resolved.push(file);
					}
					// Otherwise the file is gone from this listing (deleted/moved out)
					// and is dropped from the selection.
				}

				// Nothing resolved: keep the previous selection to avoid clearing it
				// during a transient empty/loading listing.
				if (resolved.length === 0) return prev;

				// Avoid needless state churn when nothing actually changed.
				const unchanged =
					resolved.length === prev.length &&
					resolved.every((f, i) => {
						const p = prev[i];
						return (
							p &&
							p.id === f.id &&
							p.name === f.name &&
							p.extension === f.extension &&
							sdPathsEqual(p.sd_path, f.sd_path)
						);
					});
				return unchanged ? prev : resolved;
			});

			setFocusedIndex((prevFocus) => {
				if (prevFocus !== -1) return prevFocus;
				const newFocus = files.findIndex((f) => f.id === storedIds[0]);
				return newFocus !== -1 ? newFocus : prevFocus;
			});
		},
		[storedIds]
	);

	const isRenaming = renamingFileId !== null;

	const value = useMemo(
		() => ({
			selectedFiles,
			selectedFileIds,
			isSelected,
			setSelectedFiles,
			selectFile,
			clearSelection,
			selectAll,
			focusedIndex,
			setFocusedIndex,
			moveFocus,
			// Rename state
			renamingFileId,
			startRename,
			cancelRename,
			saveRename,
			isRenaming,
			// Restore selection
			restoreSelectionFromFiles
		}),
		[
			selectedFiles,
			selectedFileIds,
			isSelected,
			setSelectedFiles,
			selectFile,
			clearSelection,
			selectAll,
			focusedIndex,
			moveFocus,
			renamingFileId,
			startRename,
			cancelRename,
			saveRename,
			isRenaming,
			restoreSelectionFromFiles
		]
	);

	return (
		<SelectionContext.Provider value={value}>
			{children}
		</SelectionContext.Provider>
	);
}

export function useSelection() {
	const context = useContext(SelectionContext);
	if (!context)
		throw new Error('useSelection must be used within SelectionProvider');
	return context;
}
