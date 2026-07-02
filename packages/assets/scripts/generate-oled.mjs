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
				'-colorspace',
				'sRGB',
				'-channel',
				'RGB',
				'-evaluate',
				'multiply',
				'0.62',
				'+channel',
				'PNG32:' + oledPng
			]);
		}

		if (avifenc && !existsSync(oledHdrAvif)) {
			const tmpDir = mkdtempSync(join(os.tmpdir(), 'spacedrive-oled-'));
			const hdrPng = join(tmpDir, `${stem}.png`);

			run(magick, [
				oledPng,
				'-alpha',
				'on',
				'-colorspace',
				'RGB',
				'-channel',
				'RGB',
				'-sigmoidal-contrast',
				'4x45%',
				'-evaluate',
				'multiply',
				'1.8',
				'+channel',
				'PNG64:' + hdrPng
			]);

			run(avifenc, [
				'-d',
				'10',
				'-y',
				'444',
				'--cicp',
				'9/16/9',
				'--range',
				'full',
				'--clli',
				'1000,400',
				'-q',
				'90',
				hdrPng,
				oledHdrAvif
			]);

			await fs.rm(tmpDir, {recursive: true, force: true});
		}
	}
}
