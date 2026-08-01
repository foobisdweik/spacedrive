import type {ContentKind, File} from '@sd/ts-client/generated/types';

export type MediaAction =
	| 'extractSubtitles'
	| 'extractText'
	| 'generateBlurhash'
	| 'generateProxy'
	| 'generateThumbstrip'
	| 'regenerateThumbnail'
	| 'transcribeAudio';

export type MediaTargetKind =
	| 'audio'
	| 'directory'
	| 'document'
	| 'image'
	| 'unknown'
	| 'pdf'
	| 'video';

export interface MediaActionTarget {
	contentKind: ContentKind;
	extension: string | null;
	isVirtual: boolean;
	kind: File['kind'];
	name: string;
}

export interface MediaActionFailure {
	error: unknown;
	name: string;
}

export function filterVisibleMediaMenuItems<
	T extends {condition?: () => boolean}
>(items: readonly T[]): T[] {
	return items.filter((item) => item.condition?.() !== false);
}

const DOCUMENT_EXTENSIONS = new Set(['doc', 'docx']);

export const MEDIA_ACTION_CAPABILITIES: Readonly<
	Record<MediaAction, readonly MediaTargetKind[]>
> = {
	extractSubtitles: ['video'],
	extractText: ['document', 'image', 'pdf'],
	generateBlurhash: ['image', 'video'],
	generateProxy: ['video'],
	generateThumbstrip: ['video'],
	regenerateThumbnail: ['image', 'pdf', 'video'],
	transcribeAudio: ['audio']
};

const MEDIA_ACTION_LABELS: Readonly<Record<MediaAction, string>> = {
	extractSubtitles: 'Extract subtitles',
	extractText: 'Extract text',
	generateBlurhash: 'Generate blurhash',
	generateProxy: 'Generate proxy',
	generateThumbstrip: 'Generate thumbstrip',
	regenerateThumbnail: 'Regenerate thumbnail',
	transcribeAudio: 'Transcribe audio'
};

const MEDIA_ACTION_REQUIREMENTS: Readonly<Record<MediaAction, string>> = {
	extractSubtitles: 'videos',
	extractText: 'images or supported documents (PDF, DOC, or DOCX)',
	generateBlurhash: 'images or videos',
	generateProxy: 'videos',
	generateThumbstrip: 'videos',
	regenerateThumbnail: 'images, videos, or PDF documents',
	transcribeAudio: 'audio files'
};

export function getEffectiveSelection<T>(
	file: T | null | undefined,
	selectedFiles: readonly T[],
	selected: boolean
): T[] {
	if (selected && selectedFiles.length > 0) return [...selectedFiles];
	return file == null ? [] : [file];
}

export function toMediaActionTarget(
	file: File,
	isVirtual: boolean
): MediaActionTarget {
	return {
		contentKind:
			file.content_identity?.kind ?? file.content_kind ?? 'unknown',
		extension: file.extension,
		isVirtual,
		kind: file.kind,
		name: file.name
	};
}

export function createAudioTranscriptionInput(entryUuid: string) {
	return {
		entry_uuid: entryUuid,
		model: 'base' as const,
		language: null
	};
}

export function getMediaTargetKind(target: MediaActionTarget): MediaTargetKind {
	if (target.kind === 'Directory') return 'directory';
	if (target.kind !== 'File' || target.isVirtual) return 'unknown';

	if (
		target.contentKind === 'image' ||
		target.contentKind === 'video' ||
		target.contentKind === 'audio'
	) {
		return target.contentKind;
	}

	const extension = target.extension?.toLowerCase();
	if (extension === 'pdf') return 'pdf';
	if (extension && DOCUMENT_EXTENSIONS.has(extension)) {
		return 'document';
	}

	return 'unknown';
}

export function isMediaActionSupported(
	action: MediaAction,
	targets: readonly MediaActionTarget[]
): boolean {
	const capabilities = MEDIA_ACTION_CAPABILITIES[action];
	return (
		targets.length > 0 &&
		targets.every((target) =>
			capabilities.includes(getMediaTargetKind(target))
		)
	);
}

export function isUniformMediaSelection(
	targets: readonly MediaActionTarget[],
	kind: Exclude<MediaTargetKind, 'directory' | 'unknown'>
): boolean {
	return (
		targets.length > 0 &&
		targets.every((target) => getMediaTargetKind(target) === kind)
	);
}

export function isDocumentMediaSelection(
	targets: readonly MediaActionTarget[]
): boolean {
	return (
		targets.length > 0 &&
		targets.every((target) => {
			const kind = getMediaTargetKind(target);
			return kind === 'document' || kind === 'pdf';
		})
	);
}

export function validateMediaAction(
	action: MediaAction,
	targets: readonly MediaActionTarget[]
): {ok: true} | {ok: false; message: string} {
	if (targets.length === 0) {
		return {
			ok: false,
			message: `${MEDIA_ACTION_LABELS[action]} requires at least one selected file.`
		};
	}

	const capabilities = MEDIA_ACTION_CAPABILITIES[action];
	const unsupported = targets.filter(
		(target) => !capabilities.includes(getMediaTargetKind(target))
	);
	if (unsupported.length === 0) return {ok: true};

	return {
		ok: false,
		message: `${MEDIA_ACTION_LABELS[action]} supports ${MEDIA_ACTION_REQUIREMENTS[action]}. Unsupported: ${summarizeTargets(unsupported)}.`
	};
}

export function formatMediaActionFailures(
	action: MediaAction,
	failures: readonly MediaActionFailure[]
): string {
	const summary = failures
		.slice(0, 3)
		.map(({error, name}) => `${name}: ${getErrorMessage(error)}`)
		.join('; ');
	const remaining =
		failures.length > 3 ? `; and ${failures.length - 3} more` : '';

	return `${MEDIA_ACTION_LABELS[action]} failed for ${failures.length} item${failures.length === 1 ? '' : 's'}: ${summary}${remaining}.`;
}

function summarizeTargets(targets: readonly MediaActionTarget[]): string {
	const summary = targets
		.slice(0, 3)
		.map((target) => `${target.name} (${describeTarget(target)})`)
		.join(', ');
	const remaining =
		targets.length > 3 ? `, and ${targets.length - 3} more` : '';
	return `${summary}${remaining}`;
}

function describeTarget(target: MediaActionTarget): string {
	if (target.isVirtual) return 'virtual item';

	const kind = getMediaTargetKind(target);
	if (kind === 'directory') return 'directory';
	if (kind === 'unknown') return 'unsupported file type';
	return kind;
}

function getErrorMessage(error: unknown): string {
	if (error instanceof Error) return error.message;
	if (typeof error === 'string') return error;
	return 'unknown error';
}
