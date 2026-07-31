import type {ContentKind, File} from '@sd/ts-client/generated/types';
import {describe, expect, test} from 'bun:test';
import {
	createAudioTranscriptionInput,
	formatMediaActionFailures,
	getEffectiveSelection,
	getMediaTargetKind,
	isDocumentMediaSelection,
	isMediaActionSupported,
	isUniformMediaSelection,
	validateMediaAction,
	type MediaActionTarget
} from '../src/routes/explorer/hooks/mediaActionCapabilities';

function target(
	name: string,
	contentKind: ContentKind,
	kind: File['kind'] = 'File',
	extension: string | null = null
): MediaActionTarget {
	return {
		contentKind,
		extension,
		isVirtual: false,
		kind,
		name
	};
}

describe('media action capabilities', () => {
	test('uses the selected files only when the clicked file is selected', () => {
		const clicked = {id: 'clicked'};
		const selected = [{id: 'one'}, {id: 'two'}];

		expect(getEffectiveSelection(clicked, selected, true)).toEqual(
			selected
		);
		expect(getEffectiveSelection(clicked, selected, false)).toEqual([
			clicked
		]);
	});

	test('classifies image, audio, video, directory, document, and unknown targets', () => {
		expect(getMediaTargetKind(target('image.png', 'image'))).toBe('image');
		expect(getMediaTargetKind(target('audio.wav', 'audio'))).toBe('audio');
		expect(getMediaTargetKind(target('video.mp4', 'video'))).toBe('video');
		expect(
			getMediaTargetKind(target('folder', 'unknown', 'Directory'))
		).toBe('directory');
		expect(
			getMediaTargetKind(target('report.PDF', 'unknown', 'File', 'PDF'))
		).toBe('pdf');
		expect(
			getMediaTargetKind(target('report.docx', 'unknown', 'File', 'docx'))
		).toBe('document');
		expect(getMediaTargetKind(target('data.bin', 'unknown'))).toBe(
			'unknown'
		);
	});

	test('allows PDF thumbnails but rejects DOC and DOCX thumbnails', () => {
		const pdf = target('report.pdf', 'document', 'File', 'pdf');
		const doc = target('report.doc', 'document', 'File', 'doc');
		const docx = target('report.docx', 'document', 'File', 'docx');

		expect(isMediaActionSupported('regenerateThumbnail', [pdf])).toBe(true);
		expect(isMediaActionSupported('regenerateThumbnail', [doc])).toBe(
			false
		);
		expect(isMediaActionSupported('regenerateThumbnail', [docx])).toBe(
			false
		);
		expect(
			isMediaActionSupported('regenerateThumbnail', [pdf, doc, docx])
		).toBe(false);
		expect(isMediaActionSupported('extractText', [pdf, doc, docx])).toBe(
			true
		);
		expect(isDocumentMediaSelection([pdf, doc, docx])).toBe(true);
	});

	test('builds audio transcription input with the accepted Whisper model', () => {
		expect(createAudioTranscriptionInput('entry-id')).toEqual({
			entry_uuid: 'entry-id',
			model: 'base',
			language: null
		});
	});

	test('exposes only actions supported by every target', () => {
		const image = target('image.png', 'image');
		const audio = target('audio.wav', 'audio');
		const video = target('video.mp4', 'video');
		const directory = target('folder', 'unknown', 'Directory');
		const unknown = target('data.bin', 'unknown');

		expect(isMediaActionSupported('extractText', [image])).toBe(true);
		expect(isMediaActionSupported('transcribeAudio', [audio])).toBe(true);
		expect(isMediaActionSupported('extractSubtitles', [video])).toBe(true);
		expect(isMediaActionSupported('generateProxy', [video])).toBe(true);
		expect(isMediaActionSupported('generateProxy', [image])).toBe(false);
		expect(isMediaActionSupported('regenerateThumbnail', [directory])).toBe(
			false
		);
		expect(isMediaActionSupported('generateBlurhash', [unknown])).toBe(
			false
		);
	});

	test('uses the capability intersection for mixed selections', () => {
		const image = target('image.png', 'image');
		const video = target('video.mp4', 'video');
		const audio = target('audio.wav', 'audio');

		expect(
			isMediaActionSupported('regenerateThumbnail', [image, video])
		).toBe(true);
		expect(isMediaActionSupported('generateBlurhash', [image, video])).toBe(
			true
		);
		expect(isMediaActionSupported('extractText', [image, video])).toBe(
			false
		);
		expect(isMediaActionSupported('generateProxy', [image, video])).toBe(
			false
		);
		expect(isMediaActionSupported('transcribeAudio', [audio, video])).toBe(
			false
		);
		expect(isMediaActionSupported('generateBlurhash', [image, audio])).toBe(
			false
		);
		expect(isUniformMediaSelection([image, video], 'image')).toBe(false);
	});

	test('rejects incompatible execution with an actionable aggregate', () => {
		const result = validateMediaAction('generateProxy', [
			target('image.png', 'image'),
			target('folder', 'unknown', 'Directory'),
			target('data.bin', 'unknown')
		]);

		expect(result.ok).toBe(false);
		if (result.ok) return;
		expect(result.message).toContain('Generate proxy supports videos');
		expect(result.message).toContain('image.png (image)');
		expect(result.message).toContain('folder (directory)');
		expect(result.message).toContain('data.bin (unsupported file type)');
	});

	test('aggregates mutation failures without losing target names', () => {
		expect(
			formatMediaActionFailures('regenerateThumbnail', [
				{name: 'one.png', error: new Error('decoder failed')},
				{name: 'two.png', error: 'permission denied'}
			])
		).toBe(
			'Regenerate thumbnail failed for 2 items: one.png: decoder failed; two.png: permission denied.'
		);
	});
});
