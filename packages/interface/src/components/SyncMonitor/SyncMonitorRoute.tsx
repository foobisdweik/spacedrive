import {
	ArrowsClockwise,
	CaretLeft,
	CheckCircle,
	ClockCounterClockwise,
	Folder,
	Pause,
	Play,
	Plus,
	Trash
} from '@phosphor-icons/react';
import type {
	DirectoryListingOutput,
	File as SdFile,
	SdPath,
	SyncConduitResponse,
	SyncGenerationResponse
} from '@sd/ts-client';
import type {ComponentType, FormEvent} from 'react';
import {useEffect, useMemo, useRef, useState} from 'react';
import {useSearchParams} from 'react-router-dom';
import {
	useLibraryMutation,
	useLibraryQuery,
	useSpacedriveClient
} from '../../contexts/SpacedriveContext';
import {ActivityFeed} from './components/ActivityFeed';
import {PeerList} from './components/PeerList';
import {useSyncMonitor} from './hooks/useSyncMonitor';

function formatBytes(bytes: number): string {
	if (bytes === 0) return '0 B';
	const units = ['B', 'KB', 'MB', 'GB', 'TB'];
	const index = Math.min(
		Math.floor(Math.log(bytes) / Math.log(1024)),
		units.length - 1
	);
	return `${(bytes / Math.pow(1024, index)).toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

function formatDate(value: string | null): string {
	if (!value) return 'Never';
	return new Intl.DateTimeFormat(undefined, {
		dateStyle: 'medium',
		timeStyle: 'short'
	}).format(new Date(value));
}

function parseEntryId(value: string): number | null {
	const parsed = Number(value);
	return Number.isInteger(parsed) && parsed > 0 ? parsed : null;
}

function physicalPath(path: SdPath | null | undefined): string | null {
	return path && 'Physical' in path ? path.Physical.path : null;
}

function comparablePhysicalPath(
	path: SdPath | null | undefined
): string | null {
	const value = physicalPath(path);
	return value?.replace(/\\/g, '/').replace(/\/+$/, '') ?? null;
}

function physicalParent(path: SdPath | null | undefined): SdPath | null {
	if (!path || !('Physical' in path)) return null;
	const normalized = path.Physical.path.replace(/[\\/]+$/, '');
	const index = Math.max(
		normalized.lastIndexOf('/'),
		normalized.lastIndexOf('\\')
	);
	if (index <= 0) return path;
	const parent =
		normalized[1] === ':' && index === 2
			? normalized.slice(0, index + 1)
			: normalized.slice(0, index);
	return {
		Physical: {
			device_slug: path.Physical.device_slug,
			path: parent
		}
	};
}

function samePhysicalPath(
	left: SdPath | null | undefined,
	right: SdPath | null | undefined
): boolean {
	return comparablePhysicalPath(left) === comparablePhysicalPath(right);
}

function statusTone(conduit: SyncConduitResponse): string {
	if (conduit.last_sync_error) return 'text-status-error';
	if (conduit.is_syncing) return 'text-accent';
	if (!conduit.enabled) return 'text-ink-faint';
	return 'text-status-success';
}

function statusLabel(conduit: SyncConduitResponse): string {
	if (conduit.last_sync_error) return 'Error';
	if (conduit.is_syncing) return 'Syncing';
	if (!conduit.enabled) return 'Paused';
	return 'Ready';
}

export function SyncMonitorRoute() {
	const sync = useSyncMonitor();
	const client = useSpacedriveClient();
	const [searchParams, setSearchParams] = useSearchParams();
	const [selectedConduitId, setSelectedConduitId] = useState<number | null>(
		null
	);
	const [sourceEntryId, setSourceEntryId] = useState('');
	const [targetEntryId, setTargetEntryId] = useState('');
	const [syncMode, setSyncMode] = useState('mirror');
	const [schedule, setSchedule] = useState('manual');
	const [useIndexRules, setUseIndexRules] = useState(false);
	const [formError, setFormError] = useState<string | null>(null);
	const [targetBrowsePath, setTargetBrowsePath] = useState<SdPath | null>(
		null
	);
	const [selectedTarget, setSelectedTarget] = useState<SdFile | null>(null);
	const [targetPickerError, setTargetPickerError] = useState<string | null>(
		null
	);
	const sourceUuid = searchParams.get('source');
	const sourceName = searchParams.get('sourceName');

	const conduitsQuery = useLibraryQuery({
		type: 'file_sync.conduit.list',
		input: {}
	});
	const conduits = useMemo(
		() => (conduitsQuery.data ?? []) as SyncConduitResponse[],
		[conduitsQuery.data]
	);
	const selectedConduit =
		conduits.find((conduit) => conduit.id === selectedConduitId) ??
		conduits[0];
	const activeConduitId = selectedConduit?.id ?? 0;

	const statusQuery = useLibraryQuery(
		{
			type: 'file_sync.status.get',
			input: {conduit_id: activeConduitId}
		},
		{
			enabled: activeConduitId > 0,
			refetchInterval: selectedConduit?.is_syncing ? 1500 : false
		}
	);
	const progressQuery = useLibraryQuery(
		{
			type: 'file_sync.status.progress',
			input: {conduit_id: activeConduitId}
		},
		{
			enabled: activeConduitId > 0,
			refetchInterval: selectedConduit?.is_syncing ? 1500 : false
		}
	);
	const historyQuery = useLibraryQuery(
		{
			type: 'file_sync.history.list',
			input: {conduit_id: activeConduitId, limit: 8}
		},
		{enabled: activeConduitId > 0}
	);
	const conflictsQuery = useLibraryQuery(
		{
			type: 'file_sync.conflicts.list',
			input: {conduit_id: activeConduitId}
		},
		{enabled: activeConduitId > 0}
	);
	const sourceFileQuery = useLibraryQuery(
		{
			type: 'files.by_id',
			input: {file_id: sourceUuid ?? ''}
		},
		{enabled: !!sourceUuid}
	);
	const sourceResolveQuery = useLibraryQuery(
		{
			type: 'files.entry.resolve',
			input: {entry_uuid: sourceUuid ?? ''}
		},
		{enabled: !!sourceUuid}
	);
	const browserPath = targetBrowsePath ?? null;
	const targetDirectoryQuery = useLibraryQuery(
		{
			type: 'files.directory_listing',
			input: {
				path:
					browserPath ??
					({
						Physical: {device_slug: '', path: '/'}
					} satisfies SdPath),
				limit: 200,
				include_hidden: false,
				sort_by: 'name',
				folders_first: true
			}
		},
		{enabled: !!browserPath}
	);
	const targetResolveQuery = useLibraryQuery(
		{
			type: 'files.entry.resolve',
			input: {entry_uuid: selectedTarget?.id ?? ''}
		},
		{enabled: !!selectedTarget}
	);

	const refetchAll = () => {
		conduitsQuery.refetch();
		statusQuery.refetch();
		progressQuery.refetch();
		historyQuery.refetch();
		conflictsQuery.refetch();
	};

	const createConduit = useLibraryMutation('file_sync.conduit.create', {
		onSuccess: refetchAll
	});
	const updateConduit = useLibraryMutation('file_sync.conduit.update', {
		onSuccess: refetchAll
	});
	const deleteConduit = useLibraryMutation('file_sync.conduit.delete', {
		onSuccess: () => {
			setSelectedConduitId(null);
			refetchAll();
		}
	});
	const syncNow = useLibraryMutation('file_sync.sync.now', {
		onSuccess: refetchAll
	});
	const pauseSync = useLibraryMutation('file_sync.sync.pause', {
		onSuccess: refetchAll
	});
	const resumeSync = useLibraryMutation('file_sync.sync.resume', {
		onSuccess: refetchAll
	});

	const refetchRef = useRef(refetchAll);
	useEffect(() => {
		refetchRef.current = refetchAll;
	});

	useEffect(() => {
		if (!client) return;

		let unsubscribe: (() => void) | undefined;
		let isCancelled = false;
		const filter = {
			event_types: [
				'JobQueued',
				'JobStarted',
				'JobProgress',
				'JobCompleted',
				'JobFailed',
				'JobCancelled',
				'FileSyncConduitChanged',
				'FileSyncStarted',
				'FileSyncProgress',
				'FileSyncCompleted',
				'FileSyncFailed'
			]
		};

		client
			.subscribeFiltered(filter, () => refetchRef.current())
			.then((unsub) => {
				if (isCancelled) {
					unsub();
				} else {
					unsubscribe = unsub;
				}
			});

		return () => {
			isCancelled = true;
			unsubscribe?.();
		};
	}, [client]);

	useEffect(() => {
		if (
			selectedConduitId &&
			conduits.some((conduit) => conduit.id === selectedConduitId)
		) {
			return;
		}
		setSelectedConduitId(conduits[0]?.id ?? null);
	}, [conduits, selectedConduitId]);

	const sourceFile = sourceFileQuery.data as SdFile | undefined;
	useEffect(() => {
		setSelectedTarget(null);
		setTargetPickerError(null);
		setTargetBrowsePath(
			sourceFile?.sd_path ? physicalParent(sourceFile.sd_path) : null
		);
	}, [sourceUuid, sourceFile?.sd_path]);

	const onlinePeerCount = sync.peers.filter((peer) => peer.isOnline).length;
	const history = (historyQuery.data ?? []) as SyncGenerationResponse[];
	const conflicts = conflictsQuery.data?.conflicts ?? [];
	const targetListing = targetDirectoryQuery.data as
		| DirectoryListingOutput
		| undefined;
	const targetDirectories = (targetListing?.files ?? []).filter(
		(file) => file.kind === 'Directory'
	);
	const parentPath = physicalParent(browserPath);
	const canBrowseUp =
		parentPath !== null && !samePhysicalPath(parentPath, browserPath);
	const totalFilesSynced = conduits.reduce(
		(total, conduit) => total + conduit.files_synced,
		0
	);
	const totalBytesTransferred = conduits.reduce(
		(total, conduit) => total + conduit.bytes_transferred,
		0
	);
	const sourceTitle = sourceFile?.name ?? sourceName ?? 'Selected folder';
	const sourcePhysicalPath = physicalPath(sourceFile?.sd_path);
	const browserPhysicalPath = physicalPath(browserPath);
	const selectedTargetPath = physicalPath(selectedTarget?.sd_path);
	const isCreating =
		createConduit.isPending ||
		updateConduit.isPending ||
		sourceResolveQuery.isLoading ||
		targetResolveQuery.isLoading;

	const handleCreateConduit = async (event: FormEvent) => {
		event.preventDefault();
		const source = parseEntryId(sourceEntryId);
		const target = parseEntryId(targetEntryId);

		if (!source || !target) {
			setFormError(
				'Source and target entry IDs must be positive numbers.'
			);
			return;
		}

		setFormError(null);
		try {
			const conduit = await createConduit.mutateAsync({
				source_entry_id: source,
				target_entry_id: target,
				sync_mode: syncMode,
				schedule
			});
			await updateConduit.mutateAsync({
				conduit_id: conduit.id,
				sync_mode: null,
				enabled: null,
				schedule: null,
				use_index_rules: useIndexRules,
				index_mode_override: null,
				parallel_transfers: null,
				bandwidth_limit_mbps: null
			});
			setSelectedConduitId(conduit.id);
			setSourceEntryId('');
			setTargetEntryId('');
		} catch (err) {
			setFormError(
				err instanceof Error ? err.message : 'Failed to create conduit'
			);
		}
	};

	const handleCreatePickedConduit = async () => {
		const source = sourceResolveQuery.data?.entry_id;
		const target = targetResolveQuery.data?.entry_id;

		if (!source || !target) {
			setTargetPickerError('Select a target directory.');
			return;
		}

		if (source === target) {
			setTargetPickerError('Pick a different target directory.');
			return;
		}

		setTargetPickerError(null);
		try {
			const conduit = await createConduit.mutateAsync({
				source_entry_id: source,
				target_entry_id: target,
				sync_mode: syncMode,
				schedule
			});
			await updateConduit.mutateAsync({
				conduit_id: conduit.id,
				sync_mode: null,
				enabled: null,
				schedule: null,
				use_index_rules: useIndexRules,
				index_mode_override: null,
				parallel_transfers: null,
				bandwidth_limit_mbps: null
			});
			setSelectedConduitId(conduit.id);
			const next = new URLSearchParams(searchParams);
			next.delete('source');
			next.delete('sourceName');
			setSearchParams(next, {replace: true});
		} catch (err) {
			setTargetPickerError(
				err instanceof Error ? err.message : 'Failed to create conduit'
			);
		}
	};

	const handleToggleEnabled = (conduit: SyncConduitResponse) => {
		updateConduit.mutate({
			conduit_id: conduit.id,
			sync_mode: null,
			enabled: !conduit.enabled,
			schedule: null,
			use_index_rules: null,
			index_mode_override: null,
			parallel_transfers: null,
			bandwidth_limit_mbps: null
		});
	};

	return (
		<div className="h-full overflow-auto p-6">
			<div className="mx-auto flex max-w-6xl flex-col gap-6">
				<header className="flex flex-wrap items-start justify-between gap-4">
					<div>
						<h1 className="text-ink text-2xl font-bold">Sync</h1>
						<div className="text-ink-dull mt-2 flex items-center gap-2 text-sm">
							<ArrowsClockwise className="size-4" weight="bold" />
							<span>{sync.currentState}</span>
						</div>
					</div>

					<div className="grid grid-cols-2 gap-2 text-right sm:grid-cols-4">
						<Metric
							label="Conduits"
							value={conduits.length.toString()}
						/>
						<Metric
							label="Online"
							value={onlinePeerCount.toString()}
						/>
						<Metric
							label="Files"
							value={totalFilesSynced.toString()}
						/>
						<Metric
							label="Bytes"
							value={formatBytes(totalBytesTransferred)}
						/>
					</div>
				</header>

				<div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
					<section className="border-app-line bg-app/60 min-h-[420px] rounded-lg border">
						<div className="border-app-line flex items-center justify-between border-b px-4 py-3">
							<div>
								<h2 className="text-ink text-sm font-semibold">
									File Sync Conduits
								</h2>
								<p className="text-ink-faint mt-0.5 text-xs">
									{conduitsQuery.isLoading
										? 'Loading'
										: `${conduits.length} configured`}
								</p>
							</div>
							<button
								type="button"
								className="border-app-line text-ink hover:bg-app-box inline-flex items-center gap-1 rounded-md border px-2 py-1 text-xs font-medium"
								onClick={() => conduitsQuery.refetch()}
							>
								<ArrowsClockwise
									className="size-3.5"
									weight="bold"
								/>
								Refresh
							</button>
						</div>

						<div className="divide-app-line divide-y">
							{conduits.length === 0 ? (
								<div className="text-ink-dull flex h-64 items-center justify-center text-sm">
									No file sync conduits configured.
								</div>
							) : (
								conduits.map((conduit) => (
									<button
										key={conduit.id}
										type="button"
										className={`hover:bg-app-box/60 flex w-full items-center justify-between gap-4 px-4 py-3 text-left ${
											conduit.id === activeConduitId
												? 'bg-app-box/70'
												: ''
										}`}
										onClick={() =>
											setSelectedConduitId(conduit.id)
										}
									>
										<div className="min-w-0">
											<div className="flex items-center gap-2">
												<span className="text-ink font-medium">
													#{conduit.id}
												</span>
												<span
													className={`text-xs font-medium ${statusTone(conduit)}`}
												>
													{statusLabel(conduit)}
												</span>
											</div>
											<div className="text-ink-dull mt-1 truncate text-xs">
												{conduit.source_entry_id} {'->'}{' '}
												{conduit.target_entry_id} ·{' '}
												{conduit.sync_mode} ·{' '}
												{conduit.schedule}
											</div>
										</div>
										<div className="flex items-center gap-2">
											<span className="text-ink-faint text-xs">
												Gen {conduit.sync_generation}
											</span>
											<span className="text-ink-faint text-xs">
												{formatBytes(
													conduit.bytes_transferred
												)}
											</span>
										</div>
									</button>
								))
							)}
						</div>
					</section>

					<aside className="flex flex-col gap-4">
						{sourceUuid && (
							<section className="border-app-line bg-app/60 rounded-lg border p-4">
								<div className="flex items-center gap-2">
									<ArrowsClockwise
										className="text-ink-dull size-4"
										weight="bold"
									/>
									<h2 className="text-ink text-sm font-semibold">
										Sync to...
									</h2>
								</div>

								<div className="mt-4 space-y-4">
									<div className="border-app-line bg-app-box rounded-md border px-3 py-2">
										<div className="text-ink truncate text-sm font-medium">
											{sourceTitle}
										</div>
										{sourcePhysicalPath && (
											<div className="text-ink-faint mt-1 truncate text-xs">
												{sourcePhysicalPath}
											</div>
										)}
									</div>

									<label className="text-ink-dull block text-xs font-medium">
										Mode
										<select
											className="border-app-line bg-app-box text-ink mt-1 w-full rounded-md border px-2 py-2 text-sm"
											value={syncMode}
											onChange={(event) =>
												setSyncMode(event.target.value)
											}
										>
											<option value="mirror">
												Mirror
											</option>
											<option value="bidirectional">
												Bidirectional
											</option>
											<option value="selective">
												Selective
											</option>
										</select>
									</label>
									<label className="text-ink-dull block text-xs font-medium">
										Schedule
										<input
											className="border-app-line bg-app-box text-ink mt-1 w-full rounded-md border px-2 py-2 text-sm"
											value={schedule}
											onChange={(event) =>
												setSchedule(event.target.value)
											}
										/>
									</label>
									<label className="text-ink-dull flex items-center gap-2 text-xs">
										<input
											type="checkbox"
											checked={useIndexRules}
											onChange={(event) =>
												setUseIndexRules(
													event.target.checked
												)
											}
										/>
										Use index filtering rules
									</label>

									<div>
										<div className="mb-2 flex items-center justify-between gap-2">
											<button
												type="button"
												className="border-app-line text-ink inline-flex items-center gap-1 rounded-md border px-2 py-1 text-xs font-medium disabled:opacity-50"
												disabled={!canBrowseUp}
												onClick={() => {
													if (parentPath)
														setTargetBrowsePath(
															parentPath
														);
												}}
											>
												<CaretLeft
													className="size-3.5"
													weight="bold"
												/>
												Back
											</button>
											<div className="text-ink-faint min-w-0 truncate text-xs">
												{browserPhysicalPath ??
													'No folder'}
											</div>
										</div>
										<div className="border-app-line bg-app-box max-h-56 overflow-y-auto rounded-md border">
											{targetDirectoryQuery.isLoading ? (
												<div className="text-ink-dull px-3 py-4 text-sm">
													Loading
												</div>
											) : targetDirectories.length ===
											  0 ? (
												<div className="text-ink-dull px-3 py-4 text-sm">
													No folders
												</div>
											) : (
												targetDirectories.map(
													(directory) => (
														<button
															key={directory.id}
															type="button"
															className={`hover:bg-app/70 flex w-full items-center gap-2 px-3 py-2 text-left text-sm ${
																selectedTarget?.id ===
																directory.id
																	? 'bg-app/80 text-accent'
																	: 'text-ink'
															}`}
															onClick={() =>
																setSelectedTarget(
																	directory
																)
															}
															onDoubleClick={() => {
																setSelectedTarget(
																	directory
																);
																setTargetBrowsePath(
																	directory.sd_path
																);
															}}
														>
															<Folder
																className="size-4 shrink-0"
																weight={
																	selectedTarget?.id ===
																	directory.id
																		? 'fill'
																		: 'regular'
																}
															/>
															<span className="min-w-0 flex-1 truncate">
																{directory.name}
															</span>
														</button>
													)
												)
											)}
										</div>
									</div>

									{selectedTarget && (
										<div className="border-app-line bg-app-box rounded-md border px-3 py-2">
											<div className="text-ink-dull truncate text-xs font-medium">
												Target
											</div>
											<div className="text-ink mt-1 truncate text-sm">
												{selectedTarget.name}
											</div>
											{selectedTargetPath && (
												<div className="text-ink-faint mt-1 truncate text-xs">
													{selectedTargetPath}
												</div>
											)}
										</div>
									)}

									{targetPickerError && (
										<div className="text-status-error text-xs">
											{targetPickerError}
										</div>
									)}

									<div className="flex gap-2">
										<button
											type="button"
											className="bg-accent flex-1 rounded-md px-3 py-2 text-sm font-semibold text-white disabled:opacity-50"
											disabled={
												isCreating || !selectedTarget
											}
											onClick={handleCreatePickedConduit}
										>
											Create
										</button>
										<button
											type="button"
											className="border-app-line text-ink hover:bg-app-box rounded-md border px-3 py-2 text-sm font-medium"
											onClick={() => {
												const next =
													new URLSearchParams(
														searchParams
													);
												next.delete('source');
												next.delete('sourceName');
												setSearchParams(next, {
													replace: true
												});
											}}
										>
											Cancel
										</button>
									</div>
								</div>
							</section>
						)}

						<section className="border-app-line bg-app/60 rounded-lg border p-4">
							<div className="flex items-center gap-2">
								<Plus
									className="text-ink-dull size-4"
									weight="bold"
								/>
								<h2 className="text-ink text-sm font-semibold">
									Advanced Conduit
								</h2>
							</div>
							<form
								className="mt-4 space-y-3"
								onSubmit={handleCreateConduit}
							>
								<Field
									label="Source entry ID"
									value={sourceEntryId}
									onChange={setSourceEntryId}
								/>
								<Field
									label="Target entry ID"
									value={targetEntryId}
									onChange={setTargetEntryId}
								/>
								<label className="text-ink-dull block text-xs font-medium">
									Mode
									<select
										className="border-app-line bg-app-box text-ink mt-1 w-full rounded-md border px-2 py-2 text-sm"
										value={syncMode}
										onChange={(event) =>
											setSyncMode(event.target.value)
										}
									>
										<option value="mirror">Mirror</option>
										<option value="bidirectional">
											Bidirectional
										</option>
										<option value="selective">
											Selective
										</option>
									</select>
								</label>
								<label className="text-ink-dull block text-xs font-medium">
									Schedule
									<input
										className="border-app-line bg-app-box text-ink mt-1 w-full rounded-md border px-2 py-2 text-sm"
										value={schedule}
										onChange={(event) =>
											setSchedule(event.target.value)
										}
									/>
								</label>
								<label className="text-ink-dull flex items-center gap-2 text-xs">
									<input
										type="checkbox"
										checked={useIndexRules}
										onChange={(event) =>
											setUseIndexRules(
												event.target.checked
											)
										}
									/>
									Use index filtering rules
								</label>
								{formError && (
									<div className="text-status-error text-xs">
										{formError}
									</div>
								)}
								<button
									type="submit"
									className="bg-accent w-full rounded-md px-3 py-2 text-sm font-semibold text-white disabled:opacity-50"
									disabled={
										createConduit.isPending ||
										updateConduit.isPending
									}
								>
									Create
								</button>
							</form>
						</section>

						{selectedConduit && (
							<ConduitActions
								conduit={selectedConduit}
								status={statusQuery.data}
								progress={progressQuery.data}
								onSync={() =>
									syncNow.mutate({
										conduit_id: selectedConduit.id
									})
								}
								onPause={() =>
									pauseSync.mutate({
										conduit_id: selectedConduit.id
									})
								}
								onResume={() =>
									resumeSync.mutate({
										conduit_id: selectedConduit.id
									})
								}
								onToggleEnabled={() =>
									handleToggleEnabled(selectedConduit)
								}
								onDelete={() =>
									deleteConduit.mutate({
										conduit_id: selectedConduit.id
									})
								}
								onRefresh={refetchAll}
							/>
						)}
					</aside>
				</div>

				<div className="grid gap-4 lg:grid-cols-2">
					<section className="border-app-line bg-app/60 rounded-lg border">
						<div className="border-app-line flex items-center gap-2 border-b px-4 py-3">
							<ClockCounterClockwise
								className="text-ink-dull size-4"
								weight="bold"
							/>
							<h2 className="text-ink text-sm font-semibold">
								History
							</h2>
						</div>
						<HistoryList history={history} />
					</section>

					<section className="border-app-line bg-app/60 rounded-lg border">
						<div className="border-app-line flex items-center justify-between border-b px-4 py-3">
							<h2 className="text-ink text-sm font-semibold">
								Conflicts & Library Sync
							</h2>
							{conflicts.length > 0 && (
								<span className="text-status-warning text-xs">
									{conflicts.length}
								</span>
							)}
						</div>
						<div className="grid gap-4 p-4 lg:grid-cols-2">
							<div>
								<h3 className="text-ink-faint mb-2 text-xs font-medium uppercase tracking-wide">
									Peers
								</h3>
								<div className="border-app-line max-h-64 overflow-y-auto rounded-md border">
									<PeerList
										peers={sync.peers}
										currentState={sync.currentState}
									/>
								</div>
							</div>
							<div>
								<h3 className="text-ink-faint mb-2 text-xs font-medium uppercase tracking-wide">
									Activity
								</h3>
								<div className="border-app-line max-h-64 overflow-y-auto rounded-md border">
									<ActivityFeed
										activities={sync.recentActivity}
									/>
								</div>
							</div>
						</div>
					</section>
				</div>
			</div>
		</div>
	);
}

function Metric({label, value}: {label: string; value: string}) {
	return (
		<div className="border-app-line bg-app-box rounded-lg border px-3 py-2">
			<div className="text-ink text-lg font-semibold">{value}</div>
			<div className="text-ink-faint text-xs">{label}</div>
		</div>
	);
}

function Field({
	label,
	value,
	onChange
}: {
	label: string;
	value: string;
	onChange: (value: string) => void;
}) {
	return (
		<label className="text-ink-dull block text-xs font-medium">
			{label}
			<input
				className="border-app-line bg-app-box text-ink mt-1 w-full rounded-md border px-2 py-2 text-sm"
				inputMode="numeric"
				value={value}
				onChange={(event) => onChange(event.target.value)}
			/>
		</label>
	);
}

function ConduitActions({
	conduit,
	status,
	progress,
	onSync,
	onPause,
	onResume,
	onToggleEnabled,
	onDelete,
	onRefresh
}: {
	conduit: SyncConduitResponse;
	status?: {
		is_syncing: boolean;
		last_sync_completed_at: string | null;
		last_sync_error: string | null;
	};
	progress?: {phase: string; is_syncing: boolean};
	onSync: () => void;
	onPause: () => void;
	onResume: () => void;
	onToggleEnabled: () => void;
	onDelete: () => void;
	onRefresh: () => void;
}) {
	return (
		<section className="border-app-line bg-app/60 rounded-lg border p-4">
			<div className="flex items-start justify-between gap-2">
				<div>
					<h2 className="text-ink text-sm font-semibold">
						Conduit #{conduit.id}
					</h2>
					<p className="text-ink-dull mt-1 text-xs">
						Last sync{' '}
						{formatDate(status?.last_sync_completed_at ?? null)}
					</p>
				</div>
				<button
					type="button"
					className="text-ink-dull hover:bg-app-box hover:text-status-error rounded-md p-1"
					onClick={onDelete}
					title="Delete conduit"
				>
					<Trash className="size-4" weight="bold" />
				</button>
			</div>

			<div className="text-ink-dull mt-4 space-y-2 text-xs">
				<Row
					label="Source"
					value={conduit.source_entry_id.toString()}
				/>
				<Row
					label="Target"
					value={conduit.target_entry_id.toString()}
				/>
				<Row
					label="Generation"
					value={conduit.sync_generation.toString()}
				/>
				<Row label="Phase" value={progress?.phase ?? 'Idle'} />
				<Row
					label="Files synced"
					value={conduit.files_synced.toString()}
				/>
				<Row
					label="Transferred"
					value={formatBytes(conduit.bytes_transferred)}
				/>
				{(status?.last_sync_error || conduit.last_sync_error) && (
					<div className="border-status-error/40 bg-status-error/10 text-status-error rounded-md border px-2 py-2">
						{status?.last_sync_error ?? conduit.last_sync_error}
					</div>
				)}
			</div>

			<div className="mt-4 grid grid-cols-2 gap-2">
				<ActionButton icon={Play} label="Sync now" onClick={onSync} />
				{status?.is_syncing || progress?.is_syncing ? (
					<ActionButton
						icon={Pause}
						label="Pause"
						onClick={onPause}
					/>
				) : (
					<ActionButton
						icon={Play}
						label="Resume"
						onClick={onResume}
					/>
				)}
				<ActionButton
					icon={conduit.enabled ? Pause : CheckCircle}
					label={conduit.enabled ? 'Disable' : 'Enable'}
					onClick={onToggleEnabled}
				/>
				<ActionButton
					icon={ArrowsClockwise}
					label="Refresh"
					onClick={onRefresh}
				/>
			</div>
		</section>
	);
}

function Row({label, value}: {label: string; value: string}) {
	return (
		<div className="flex items-center justify-between gap-3">
			<span>{label}</span>
			<span className="text-ink truncate text-right font-medium">
				{value}
			</span>
		</div>
	);
}

function ActionButton({
	icon: Icon,
	label,
	onClick
}: {
	icon: ComponentType<{className?: string; weight?: 'bold'}>;
	label: string;
	onClick: () => void;
}) {
	return (
		<button
			type="button"
			className="border-app-line text-ink hover:bg-app-box inline-flex items-center justify-center gap-2 rounded-md border px-2 py-2 text-xs font-medium"
			onClick={onClick}
		>
			<Icon className="size-3.5" weight="bold" />
			{label}
		</button>
	);
}

function HistoryList({history}: {history: SyncGenerationResponse[]}) {
	if (history.length === 0) {
		return (
			<div className="text-ink-dull flex h-48 items-center justify-center text-sm">
				No sync history for this conduit.
			</div>
		);
	}

	return (
		<div className="divide-app-line divide-y">
			{history.map((generation) => (
				<div
					key={generation.id}
					className="flex items-center justify-between gap-4 px-4 py-3"
				>
					<div className="min-w-0">
						<div className="flex items-center gap-2">
							<span className="text-ink font-medium">
								Generation {generation.generation}
							</span>
							<span className="text-ink-faint text-xs">
								{generation.verification_status}
							</span>
						</div>
						<div className="text-ink-dull mt-1 text-xs">
							{formatDate(generation.started_at)}
						</div>
					</div>
					<div className="text-ink-dull text-right text-xs">
						<div>{generation.files_copied} copied</div>
						<div>{generation.files_deleted} deleted</div>
					</div>
				</div>
			))}
		</div>
	);
}
