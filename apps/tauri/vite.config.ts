import fs from 'fs';
import {createRequire} from 'node:module';
import path from 'path';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react-swc';
import {defineConfig} from 'vite';

const require = createRequire(import.meta.url);
const spaceui = path.resolve(__dirname, '../../../spaceui/packages');
const hasSpaceui = fs.existsSync(spaceui);
const spacebot = path.resolve(__dirname, '../../../spacebot/packages');
const hasSpacebot = fs.existsSync(spacebot);
const bunNodeModule = (pkg: string) =>
	path.dirname(require.resolve(`${pkg}/package.json`));

export default defineConfig(() => ({
	plugins: [react(), tailwindcss()],

	resolve: {
		dedupe: ['react', 'react-dom'],
		alias: [
			{
				find: /^react$/,
				replacement: path.resolve(
					__dirname,
					'./node_modules/react/index.js'
				)
			},
			{
				find: /^react\/jsx-runtime$/,
				replacement: path.resolve(
					__dirname,
					'./node_modules/react/jsx-runtime.js'
				)
			},
			{
				find: /^react\/jsx-dev-runtime$/,
				replacement: path.resolve(
					__dirname,
					'./node_modules/react/jsx-dev-runtime.js'
				)
			},
			{
				find: /^react-dom$/,
				replacement: path.resolve(
					__dirname,
					'./node_modules/react-dom/index.js'
				)
			},
			{
				find: /^react-dom\/client$/,
				replacement: path.resolve(
					__dirname,
					'./node_modules/react-dom/client.js'
				)
			},
			{
				find: 'openapi-fetch',
				replacement: path.resolve(
					__dirname,
					'../../packages/interface/node_modules/openapi-fetch/dist/index.mjs'
				)
			},
			{
				find: 'style-to-js',
				replacement: bunNodeModule('style-to-js')
			},
			{
				find: 'debug',
				replacement: bunNodeModule('debug')
			},
			{
				find: 'extend',
				replacement: bunNodeModule('extend')
			},
			{
				find: 'hast-util-to-jsx-runtime',
				replacement: bunNodeModule('hast-util-to-jsx-runtime')
			},
			{
				find: 'micromark',
				replacement: bunNodeModule('micromark')
			},
			{
				find: 'react-markdown',
				replacement: bunNodeModule('react-markdown')
			},
			{
				find: 'rehype-raw',
				replacement: bunNodeModule('rehype-raw')
			},
			{
				find: 'remark-gfm',
				replacement: bunNodeModule('remark-gfm')
			},
			{
				find: 'unified',
				replacement: bunNodeModule('unified')
			},
			// SpaceUI — resolve to source for HMR when available locally
			...(hasSpaceui
				? [
						{
							find: /^@spacedrive\/tokens\/css\/themes\/(.+)$/,
							replacement: `${spaceui}/tokens/src/css/themes/$1.css`
						},
						{
							find: /^@spacedrive\/tokens\/theme$/,
							replacement: `${spaceui}/tokens/src/css/theme.css`
						},
						{
							find: /^@spacedrive\/tokens\/css$/,
							replacement: `${spaceui}/tokens/src/css/base.css`
						},
						{
							find: /^@spacedrive\/tokens$/,
							replacement: `${spaceui}/tokens`
						},
						{
							find: /^@spacedrive\/ai$/,
							replacement: `${spaceui}/ai/src/index.ts`
						},
						{
							find: /^@spacedrive\/primitives$/,
							replacement: `${spaceui}/primitives/src/index.ts`
						}
					]
				: []),
			...(hasSpacebot
				? [
						{
							find: /^@spacebot\/api-client$/,
							replacement: `${spacebot}/api-client/src`
						}
					]
				: [
						{
							find: /^@spacebot\/api-client$/,
							replacement: path.resolve(
								__dirname,
								'./src/spacebot-api-client.ts'
							)
						}
					]),
			{
				find: '@sd/interface',
				replacement: path.resolve(
					__dirname,
					'../../packages/interface/src'
				)
			},
			{
				find: '@sd/ts-client',
				replacement: path.resolve(
					__dirname,
					'../../packages/ts-client/src'
				)
			}
		]
	},

	optimizeDeps: {
		include: [
			'debug',
			'extend',
			'hast-util-to-jsx-runtime',
			'micromark',
			'react-markdown',
			'rehype-raw',
			'remark-gfm',
			'style-to-js',
			'unified'
		],
		exclude: [
			'@spacedrive/ai',
			'@spacedrive/primitives',
			'@spacedrive/tokens'
		]
	},

	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		fs: {
			allow: [
				path.resolve(__dirname, '../../..'),
				...(hasSpaceui ? [spaceui] : [])
			]
		},
		watch: {
			ignored: ['**/src-tauri/**']
		}
	},
	envPrefix: ['VITE_', 'TAURI_ENV_*'],
	build: {
		target: ['es2021', 'chrome100', 'safari13'],
		minify: !process.env.TAURI_ENV_DEBUG ? ('esbuild' as const) : false,
		sourcemap: !!process.env.TAURI_ENV_DEBUG,
		rollupOptions: {}
	}
}));
