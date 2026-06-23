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

export function SelectionProvider({
	children,
	isActiveTab = true
}: SelectionProviderProps) {
	const platform = usePlatform();
	const clipboard = useClipboard();
	const tabManager = useTabManager();
	const queryClient = useQueryClient();
	const {activeTabId, getSelectionIds, updateSelectionIds} = tabManager;
	const renameFile = useLibraryMutation('files.rename');

	// Local state for File objects (not serializable, can't be stored in TabManager)
	const [selectedFiles, setSelectedFilesInternal] = useState<File[]>([]);
	const [focusedIndex, setFocusedIndex] = useState(-1);
	const [, setLastSelectedIndex] = useState(-1);
	const [renamingFileId, setRenamingFileId] = useState<string | null>(null);

	// Track the stored IDs for the active tab (separate from File objects)
	const storedIds = getSelectionIds(activeTabId);

	// Clear selection when activeTabId changes (we'll restore it when files load)
	useEffect(() => {
		setSelectedFilesInternal([]);
		setFocusedIndex(-1);
		setLastSelectedIndex(-1);
	}, [activeTabId]);

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
					nextFiles.map((f) => f.id)
				);

				return nextFiles;
			});
		},
		[activeTabId, updateSelectionIds]
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
				// Stale invalidation is still triggered immediately, but the job completion
				// event in useJobsDesktop.ts will perform the final authoritative invalidation.
				await queryClient.invalidateQueries({
					predicate: (query) =>
						Array.isArray(query.queryKey) &&
						typeof query.queryKey[0] === 'string' &&
						(query.queryKey[0] ===
							'query:files.directory_listing' ||
							query.queryKey[0] === 'query:search.files')
				});
			} catch (error) {
				// Keep in edit mode on error so user can retry
				console.error('Rename failed:', error);
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

			const fileMap = new Map(files.map((f) => [f.id, f]));
			const matchingFiles: File[] = [];

			for (const id of storedIds) {
				const file = fileMap.get(id);
				if (file) {
					matchingFiles.push(file);
				}
			}

			// Only update if we found matching files and they're different from current
			if (matchingFiles.length > 0) {
				setSelectedFilesInternal((prev) => {
					const prevIds = new Set(prev.map((f) => f.id));
					const newIds = new Set(matchingFiles.map((f) => f.id));

					if (
						prevIds.size === newIds.size &&
						[...newIds].every((id) => prevIds.has(id))
					) {
						const prevById = new Map(prev.map((f) => [f.id, f]));
						const hasStaleData = matchingFiles.some((file) => {
							const previous = prevById.get(file.id);
							return (
								!previous ||
								previous.name !== file.name ||
								previous.extension !== file.extension ||
								!sdPathsEqual(previous.sd_path, file.sd_path)
							);
						});
						if (!hasStaleData) {
							return prev;
						}
					}

					return matchingFiles;
				});
			}
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
