import {
	ArrowRight,
	ArrowsLeftRight,
	CircleNotch,
	Copy as CopyIcon,
	Files,
	FolderOpen,
	Warning
} from '@phosphor-icons/react';
import type {
	FileOperationPreflightOutput,
	File as FileType,
	FileValidationActionOutput,
	SdPath
} from '@sd/ts-client';
import {
	Dialog,
	dialogManager,
	useDialog,
	type UseDialogProps
} from '@spacedrive/primitives';
import {useEffect, useRef, useState} from 'react';
import {useForm} from 'react-hook-form';
import {
	useLibraryMutation,
	useLibraryQuery
} from '../../contexts/SpacedriveContext';
import {File, FileStack} from '../../routes/explorer/File';

interface FileOperationDialogProps {
	id: number;
	operation: 'copy' | 'move';
	sources: SdPath[];
	destination: SdPath;
	onComplete?: (operation: 'copy' | 'move') => void;
}

type ConflictResolution = 'Overwrite' | 'AutoModifyName' | 'Skip' | 'Abort';

type DialogPhase =
	| {type: 'validating'}
	| {type: 'form'}
	| {type: 'executing'}
	| {type: 'error'; message: string};

export function useFileOperationDialog() {
	return (options: Omit<FileOperationDialogProps, 'id'>) => {
		return dialogManager.create((props: UseDialogProps) => (
			<FileOperationDialog
				{...(props as FileOperationDialogProps)}
				{...options}
			/>
		));
	};
}

function FileOperationDialog(props: FileOperationDialogProps) {
	const dialog = useDialog(props);
	const form = useForm();
	const [phase, setPhase] = useState<DialogPhase>({type: 'validating'});
	const [preflight, setPreflight] =
		useState<FileOperationPreflightOutput | null>(null);
	const [operation, setOperation] = useState<'copy' | 'move'>(
		props.operation
	);
	const [conflictResolution, setConflictResolution] =
		useState<ConflictResolution>('AutoModifyName');
	const hasSelectedConflictResolution = useRef(false);

	const validateFiles = useLibraryMutation('files.validation');
	const copyFiles = useLibraryMutation('files.copy');
	const moveFiles = useLibraryMutation('files.move');

	// Fetch file info for sources (up to 3 for FileStack)
	const sourcePaths = props.sources
		.slice(0, 3)
		.map((s) => ('Physical' in s ? s.Physical.path : null))
		.filter((p): p is string => p !== null);

	const sourceFileQuery0 = useLibraryQuery(
		{type: 'files.by_path', input: {path: sourcePaths[0] ?? ''}},
		{enabled: !!sourcePaths[0]}
	);
	const sourceFileQuery1 = useLibraryQuery(
		{type: 'files.by_path', input: {path: sourcePaths[1] ?? ''}},
		{enabled: !!sourcePaths[1]}
	);
	const sourceFileQuery2 = useLibraryQuery(
		{type: 'files.by_path', input: {path: sourcePaths[2] ?? ''}},
		{enabled: !!sourcePaths[2]}
	);

	const sourceFiles = [sourceFileQuery0, sourceFileQuery1, sourceFileQuery2]
		.map((q) => q.data)
		.filter((f): f is FileType => f !== undefined && f !== null);

	// Fetch destination folder info
	const destPath: string | null =
		'Physical' in props.destination
			? props.destination.Physical.path
			: null;

	const {data: destFile} = useLibraryQuery(
		{type: 'files.by_path', input: {path: destPath!}},
		{enabled: !!destPath}
	);

	// Check if any source is the same as, or contains, the destination.
	const hasSameSourceDest = props.sources.some((source) => {
		return isSameOrNestedPhysicalDestination(source, props.destination);
	});

	const executeOperation = async (
		resolution: ConflictResolution = conflictResolution
	) => {
		try {
			setPhase({type: 'executing'});

			if (operation === 'move') {
				await moveFiles.mutateAsync({
					sources: {paths: props.sources},
					destination: props.destination,
					overwrite: resolution === 'Overwrite',
					verify_checksum: false,
					preserve_timestamps: true,
					copy_method: 'Auto',
					on_conflict: resolution
				});
			} else {
				await copyFiles.mutateAsync({
					sources: {paths: props.sources},
					destination: props.destination,
					overwrite: resolution === 'Overwrite',
					verify_checksum: false,
					preserve_timestamps: true,
					move_files: false,
					copy_method: 'Auto',
					on_conflict: resolution
				});
			}

			dialogManager.setState(props.id, {open: false});
			props.onComplete?.(operation);
		} catch (error) {
			setPhase({
				type: 'error',
				message:
					error instanceof Error ? error.message : 'Operation failed'
			});
		}
	};

	// Preflight validation
	useEffect(() => {
		if (hasSameSourceDest) {
			setPhase({
				type: 'error',
				message:
					'The selected item cannot be copied or moved into itself.'
			});
			return;
		}

		let isActive = true;
		setPhase({type: 'validating'});

		validateFiles
			.mutateAsync({
				preflight: {
					sources: {paths: props.sources},
					destination: props.destination,
					operation: operation === 'copy' ? 'Copy' : 'Move'
				},
				paths: [],
				verify_checksums: false,
				deep_scan: false
			})
			.then((res: FileValidationActionOutput) => {
				if (!isActive) return;
				if (isPreflightResult(res)) {
					setPreflight(res.Preflight);

					if (res.Preflight.issues.length > 0) {
						setPhase({
							type: 'error',
							message: res.Preflight.issues[0].message
						});
					} else {
						if (
							res.Preflight.requires_confirmation &&
							!hasSelectedConflictResolution.current
						) {
							setConflictResolution('AutoModifyName');
						}
						setPhase({type: 'form'});
					}
				} else {
					setPhase({
						type: 'error',
						message: 'Unexpected validation output'
					});
				}
			})
			.catch((err) => {
				if (isActive) {
					setPhase({
						type: 'error',
						message:
							err instanceof Error ? err.message : String(err)
					});
				}
			});

		return () => {
			isActive = false;
		};
	}, [operation, props.sources, props.destination, hasSameSourceDest]);

	const handleSubmit = async () => {
		await executeOperation();
	};

	const handleCancel = () => {
		dialogManager.setState(props.id, {open: false});
	};

	// Keyboard shortcuts
	useEffect(() => {
		if (phase.type !== 'form') return;

		const handleKeyDown = (e: KeyboardEvent) => {
			// Enter - Submit
			if (e.key === 'Enter' && !e.shiftKey) {
				e.preventDefault();
				handleSubmit();
				return;
			}

			// Only handle other shortcuts if not typing in an input
			if ((e.target as HTMLElement)?.tagName === 'INPUT') return;

			const key = e.key.toLowerCase();

			// ⌘1 / Ctrl+1 - Copy mode
			if ((e.metaKey || e.ctrlKey) && e.key === '1') {
				e.preventDefault();
				e.stopPropagation();
				setOperation('copy');
			}
			// ⌘2 / Ctrl+2 - Move mode
			if ((e.metaKey || e.ctrlKey) && e.key === '2') {
				e.preventDefault();
				e.stopPropagation();
				setOperation('move');
			}
			// S - Skip
			if (key === 's' && !e.metaKey && !e.ctrlKey) {
				e.preventDefault();
				hasSelectedConflictResolution.current = true;
				setConflictResolution('Skip');
			}
			// K - Keep both
			if (key === 'k' && !e.metaKey && !e.ctrlKey) {
				e.preventDefault();
				hasSelectedConflictResolution.current = true;
				setConflictResolution('AutoModifyName');
			}
			// O - Overwrite
			if (key === 'o' && !e.metaKey && !e.ctrlKey) {
				e.preventDefault();
				hasSelectedConflictResolution.current = true;
				setConflictResolution('Overwrite');
			}
		};

		window.addEventListener('keydown', handleKeyDown);
		return () => window.removeEventListener('keydown', handleKeyDown);
	}, [phase.type, operation, conflictResolution]);

	// Validating state
	if (phase.type === 'validating') {
		return (
			<Dialog
				dialog={dialog}
				form={form}
				title={
					operation === 'copy' ? 'Validating Copy' : 'Validating Move'
				}
				icon={<Files size={20} weight="bold" />}
				hideButtons
			>
				<div className="space-y-3 py-6">
					<div className="flex items-center justify-center gap-3">
						<CircleNotch
							className="text-accent size-6 animate-spin"
							weight="bold"
						/>
						<span className="text-ink text-sm">
							Checking files...
						</span>
					</div>
				</div>
			</Dialog>
		);
	}

	// Executing state
	if (phase.type === 'executing') {
		return (
			<Dialog
				dialog={dialog}
				form={form}
				title={operation === 'copy' ? 'Copying Files' : 'Moving Files'}
				icon={<Files size={20} weight="bold" />}
				hideButtons
			>
				<div className="space-y-3 py-6">
					<div className="flex items-center justify-center gap-3">
						<CircleNotch
							className="text-accent size-6 animate-spin"
							weight="bold"
						/>
						<span className="text-ink text-sm">
							{operation === 'copy'
								? 'Copying files...'
								: 'Moving files...'}
						</span>
					</div>
				</div>
			</Dialog>
		);
	}

	// Error state
	if (phase.type === 'error') {
		return (
			<Dialog
				dialog={dialog}
				form={form}
				title="Operation Failed"
				icon={
					<Warning size={20} weight="fill" className="text-red-500" />
				}
				ctaLabel="Close"
				onSubmit={form.handleSubmit(handleCancel)}
			>
				<div className="flex flex-col gap-4 py-4">
					<div className="flex items-start gap-2 rounded-md border border-red-500/20 bg-red-500/10 p-3">
						<Warning
							className="mt-0.5 size-5 text-red-500"
							weight="fill"
						/>
						<div className="flex-1">
							<div className="text-ink mb-1 text-sm font-medium">
								Error
							</div>
							<div className="text-ink-dull text-xs">
								{phase.message}
							</div>
						</div>
					</div>
				</div>
			</Dialog>
		);
	}

	const sourceCount = props.sources.length;
	const pluralItems = sourceCount === 1 ? 'item' : 'items';
	const conflictOptions = preflight?.requires_confirmation ?? true;

	// Form state - let user choose operation and conflict resolution
	return (
		<Dialog
			dialog={dialog}
			form={form}
			title="File Operation"
			icon={<Files size={20} weight="bold" />}
			ctaLabel={operation === 'copy' ? 'Copy' : 'Move'}
			onSubmit={form.handleSubmit(handleSubmit)}
			onCancelled={handleCancel}
			formClassName="!min-w-[400px] !max-w-[400px]"
		>
			<div className="space-y-5 py-2">
				{/* Source → Destination visual */}
				<div className="flex items-center gap-4">
					{/* Source */}
					<div className="flex min-w-0 flex-1 flex-col items-center gap-2">
						{sourceFiles.length > 0 ? (
							<>
								{sourceFiles.length === 1 ? (
									<File.Thumb
										file={sourceFiles[0]}
										size={80}
									/>
								) : (
									<FileStack files={sourceFiles} size={80} />
								)}
								<div className="w-full text-center">
									<div className="text-ink-dull mb-0.5 text-xs">
										Source
									</div>
									{sourceFiles.length === 1 ? (
										<div className="text-ink w-full truncate text-sm font-medium">
											{sourceFiles[0].name}
										</div>
									) : (
										<div className="text-ink text-sm font-medium">
											{preflight
												? preflight.file_count
												: sourceCount}{' '}
											{pluralItems}
										</div>
									)}
								</div>
							</>
						) : (
							<>
								<Files
									className="text-ink-dull size-20"
									weight="fill"
								/>
								<div className="text-center">
									<div className="text-ink-dull mb-0.5 text-xs">
										Source
									</div>
									<div className="text-ink text-sm font-medium">
										{preflight
											? preflight.file_count
											: sourceCount}{' '}
										{pluralItems}
									</div>
								</div>
							</>
						)}
					</div>

					{/* Arrow */}
					<div className="flex-shrink-0">
						<ArrowRight
							className="text-accent size-6"
							weight="bold"
						/>
					</div>

					{/* Destination */}
					<div className="flex min-w-0 flex-1 flex-col items-center gap-2">
						{destFile ? (
							<>
								<File.Thumb file={destFile} size={80} />
								<div className="w-full text-center">
									<div className="text-ink-dull mb-0.5 text-xs">
										To
									</div>
									<div className="text-ink w-full truncate text-sm font-medium">
										{destFile.name}
									</div>
								</div>
							</>
						) : (
							<>
								<FolderOpen
									className="text-accent size-20"
									weight="fill"
								/>
								<div className="text-center">
									<div className="text-ink-dull mb-0.5 text-xs">
										To
									</div>
									<div className="text-ink max-w-full truncate text-sm font-medium">
										{getFileName(props.destination)}
									</div>
								</div>
							</>
						)}
					</div>
				</div>

				{/* Operation type selection */}
				<div className="space-y-2">
					<div className="text-ink-dull mb-2 text-xs font-medium">
						Operation:
					</div>
					<div className="flex gap-2">
						<button
							type="button"
							onClick={() => setOperation('copy')}
							className={`flex flex-1 items-center justify-center gap-2 rounded-md px-3 py-2 text-sm font-medium transition-colors ${
								operation === 'copy'
									? 'bg-accent text-white'
									: 'bg-app-box text-ink hover:bg-app-hover'
							}`}
						>
							<CopyIcon className="size-4" weight="bold" />
							Copy
							<span className="text-xs opacity-60">⌘1</span>
						</button>
						<button
							type="button"
							onClick={() => setOperation('move')}
							className={`flex flex-1 items-center justify-center gap-2 rounded-md px-3 py-2 text-sm font-medium transition-colors ${
								operation === 'move'
									? 'bg-accent text-white'
									: 'bg-app-box text-ink hover:bg-app-hover'
							}`}
						>
							<ArrowsLeftRight className="size-4" weight="bold" />
							Move
							<span className="text-xs opacity-60">⌘2</span>
						</button>
					</div>
				</div>

				{/* Conflict resolution options */}
				{conflictOptions && (
					<div className="space-y-2">
						<div className="text-ink-dull mb-2 text-xs font-medium">
							If files already exist:
						</div>
						<div className="space-y-1">
							{[
								{
									value: 'Skip',
									label: 'Skip existing files',
									key: 'S'
								},
								{
									value: 'AutoModifyName',
									label: 'Keep both (rename new files)',
									key: 'K'
								},
								{
									value: 'Overwrite',
									label: 'Overwrite existing files',
									key: 'O'
								}
							].map((option) => (
								<label
									key={option.value}
									className="hover:bg-app-hover flex cursor-pointer items-center justify-between gap-2 rounded-md px-2 py-2 transition-colors"
								>
									<div className="flex items-center gap-2">
										<input
											type="radio"
											name="conflict-resolution"
											value={option.value}
											checked={
												conflictResolution ===
												option.value
											}
											onChange={() => {
												hasSelectedConflictResolution.current = true;
												setConflictResolution(
													option.value as ConflictResolution
												);
											}}
											className="accent-accent size-4 cursor-pointer"
										/>
										<span className="text-ink text-sm">
											{option.label}
										</span>
									</div>
									<span className="text-ink-faint text-xs font-medium">
										{option.key}
									</span>
								</label>
							))}
						</div>
					</div>
				)}
			</div>
		</Dialog>
	);
}

// Utility functions
function getFileName(path: SdPath): string {
	if (!path || typeof path !== 'object') {
		return 'Unknown';
	}

	if ('Physical' in path && path.Physical) {
		const pathStr = path.Physical.path || '';
		const parts = pathStr.split(/[\\/]/);
		return parts[parts.length - 1] || pathStr;
	}

	if ('Cloud' in path && path.Cloud) {
		const pathStr = path.Cloud.path || '';
		const parts = pathStr.split('/');
		return parts[parts.length - 1] || pathStr;
	}

	return 'Unknown';
}

function isSameOrNestedPhysicalDestination(
	source: SdPath,
	destination: SdPath
): boolean {
	if (!('Physical' in source) || !('Physical' in destination)) return false;
	if (source.Physical.device_slug !== destination.Physical.device_slug)
		return false;

	const sourcePath = normalizePathForComparison(source.Physical.path);
	const destinationPath = normalizePathForComparison(
		destination.Physical.path
	);
	const nestedPrefix = sourcePath === '/' ? '/' : `${sourcePath}/`;

	return (
		destinationPath === sourcePath ||
		destinationPath.startsWith(nestedPrefix)
	);
}

function normalizePathForComparison(path: string): string {
	const normalized = path.replace(/\\/g, '/').replace(/\/+$/, '');
	return normalized || '/';
}

function isPreflightResult(
	result: FileValidationActionOutput
): result is {Preflight: FileOperationPreflightOutput} {
	return 'Preflight' in result;
}
