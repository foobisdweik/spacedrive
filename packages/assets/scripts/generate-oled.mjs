import {spawnSync} from 'node:child_process';
import {existsSync, mkdtempSync} from 'node:fs';
import fs from 'node:fs/promises';
import os from 'node:os';
import {basename, dirname, extname, join} from 'node:path';
import {fileURLToPath} from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const assetRoot = join(__dirname, '..');
const folders = ['icons', 'images'];
const sourceExtensions = new Set(['.png']);

const commandExists = (command) =>
	spawnSync('zsh', ['-lc', `command -v ${command}`], {stdio: 'ignore'})
		.status === 0;

/*
 * Icon art is drawn for light backgrounds: the document family's body sits
 * around #232635 — hsl(230, 20%, 17%), the design system's own 235° hue family
 * at dark-theme surface lightness. On a true-black OLED panel that is 1.4:1,
 * below the 3:1 WCAG floor for non-text graphics before any processing runs.
 * Darkening (what this script used to do) drove it to 1.2:1 and the icons
 * vanished outright.
 *
 * So lift instead of crush. `+level FLOOR%,100%` maps input 0 to FLOOR and holds
 * 1 at 1, i.e. `out = floor + in * (1 - floor)`: dark fills clear the threshold
 * while highlights stay put, so already-bright icons are left alone rather than
 * washing out.
 *
 * The channel that lift runs on matters more than the floor value:
 *
 *   per-RGB  lifts each channel independently, which drags the three toward each
 *            other and desaturates. #232635 -> #585A65 clears 3:1 but collapses
 *            hsl saturation 20% -> 7%, so the whole set reads as flat grey and
 *            loses the 235° family identity.
 *   HSL L    preserves hue and saturation but HSL "lightness" is not luminance —
 *            a saturated dark blue at L=37% still carries almost none. Document
 *            lands at 4.3% of pixels clearing 3:1. Looks right, fails the job.
 *   LCH L    perceptual lightness, so lifting it buys real luminance while hue
 *            and chroma ride along untouched. This is the one that satisfies
 *            both constraints.
 *
 * 32% is the smallest LCH floor carrying the worst case clear of 3:1 with margin
 * (#232635 -> #616475, 3.59:1). Verified no sRGB gamut clipping and hue held to
 * within 1° on the bright icons (Package 34°, Key 205° -> 204°).
 */
const LIGHTNESS_FLOOR_PERCENT = 32;

const magick = commandExists('magick') ? 'magick' : null;
const avifenc = commandExists('avifenc') ? 'avifenc' : null;

if (!magick) {
	throw new Error(
		'ImageMagick `magick` is required to generate OLED assets.'
	);
}

const run = (command, args) => {
	const result = spawnSync(command, args, {stdio: 'inherit'});
	if (result.status !== 0) {
		throw new Error(
			`${command} ${args.join(' ')} failed with status ${result.status}`
		);
	}
};

const isGeneratedVariant = (fileName) =>
	/_OLED(?:_HDR)?\.(?:png|avif)$/i.test(fileName);

for (const folder of folders) {
	const folderPath = join(assetRoot, folder);
	const entries = await fs.readdir(folderPath);

	for (const fileName of entries) {
		const extension = extname(fileName).toLowerCase();
		if (!sourceExtensions.has(extension) || isGeneratedVariant(fileName))
			continue;

		const input = join(folderPath, fileName);
		const stem = basename(fileName, extension);
		const oledPng = join(folderPath, `${stem}_OLED.png`);
		const oledHdrAvif = join(folderPath, `${stem}_OLED_HDR.avif`);

		if (!existsSync(oledPng)) {
			run(magick, [
				input,
				'-alpha',
				'on',
				// In ImageMagick's LCH colorspace the R channel carries
				// perceptual lightness; chroma and hue are left untouched.
				'-colorspace',
				'LCH',
				'-channel',
				'R',
				'+level',
				`${LIGHTNESS_FLOOR_PERCENT}%,100%`,
				'+channel',
				'-colorspace',
				'sRGB',
				// The lift also raises RGB under fully-transparent pixels; left
				// alone those bleed a grey halo when the browser scales the icon
				// down, so flatten them back to black.
				'-background',
				'black',
				'-alpha',
				'background',
				'PNG32:' + oledPng
			]);
		}

		if (avifenc && !existsSync(oledHdrAvif)) {
			const tmpDir = mkdtempSync(join(os.tmpdir(), 'spacedrive-oled-'));
			const hdrPng = join(tmpDir, `${stem}.png`);

			/*
			 * This variant used to be tagged `--cicp 9/16/9` (BT.2020 primaries,
			 * PQ transfer) while the pixels were only ever sigmoidal-contrasted
			 * sRGB — nothing converted them to BT.2020 or PQ-encoded them. Players
			 * trusted the tag, so sRGB primaries got stretched across BT.2020 and
			 * the icons came back wrong: blues turned fluorescent, Package's tan
			 * #F8EAD8 skewed red. On an HDR panel this is the variant actually
			 * shown, which is why OLED mode looked worse than a plain darkening.
			 *
			 * Real PQ output needs a genuine linear -> BT.2020 -> ST.2084 pipeline,
			 * which neither ImageMagick nor avifenc will do for us. Until that
			 * exists, emit an honestly-tagged sRGB AVIF: no extra headroom, but
			 * the colours are correct.
			 */
			run(magick, [oledPng, '-alpha', 'on', '-colorspace', 'sRGB', 'PNG32:' + hdrPng]);

			run(avifenc, [
				'-d',
				'8',
				'-y',
				'444',
				'--cicp',
				'1/13/1',
				'--range',
				'full',
				'-q',
				'90',
				hdrPng,
				oledHdrAvif
			]);

			await fs.rm(tmpDir, {recursive: true, force: true});
		}
	}
}
