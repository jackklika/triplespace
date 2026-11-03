// The site's assets (ADR 0034 §8): Codex's CSS-only components, the type sets'
// typefaces, the site's own styles and the editing components (Vue, loaded lazily),
// built into hashed files with a manifest that triplespace-ui reads to name them in each
// page. The release build embeds ui/dist into
// the binaries that serve the site (rust-embed); Node is a build-time dependency only.
import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';

/**
 * The npm packages whose files end up in the build, as `packages.json` beside the
 * manifest: the frontend packages `cargo xtask manifest` lists on Special:Version with
 * their licences (ADR 0077 §6, §15). A package that is only a build tool, such as Vue's
 * compiler, is not in it.
 *
 * @return {Object} The plugin.
 */
function bundledPackages() {
	const nameOf = ( id ) => {
		const path = id.replace( /\\/g, '/' );
		const at = path.lastIndexOf( 'node_modules/' );
		if ( at < 0 ) {
			return null;
		}
		const parts = path.slice( at + 'node_modules/'.length ).split( '/' );
		return parts[ 0 ].startsWith( '@' ) ? parts.slice( 0, 2 ).join( '/' ) : parts[ 0 ];
	};
	return {
		name: 'triplespace-bundled-packages',
		generateBundle( options, bundle ) {
			const names = new Set();
			for ( const item of Object.values( bundle ) ) {
				let ids;
				if ( item.type === 'chunk' ) {
					ids = item.moduleIds || Object.keys( item.modules || {} );
				} else if ( item.originalFileNames ) {
					ids = item.originalFileNames;
				} else {
					ids = item.originalFileName ? [ item.originalFileName ] : [];
				}
				for ( const id of ids ) {
					const name = nameOf( id );
					if ( name ) {
						names.add( name );
					}
				}
			}
			this.emitFile( {
				type: 'asset',
				fileName: 'packages.json',
				source: JSON.stringify( [ ...names ].sort(), null, '\t' ) + '\n'
			} );
		}
	};
}

/**
 * Wraps the head script in a function, so that a classic <script> leaves nothing in the
 * page's global scope but the `tsAppearance` it sets on purpose. In `renderChunk`, so the
 * file's hash is of what is served.
 *
 * @return {Object} The plugin.
 */
function classicHeadScript() {
	return {
		name: 'triplespace-classic-head-script',
		renderChunk( code, chunk ) {
			if ( chunk.name !== 'head' ) {
				return null;
			}
			if ( /^\s*(import|export)\b/m.test( code ) ) {
				this.error( 'the head script must not import or export at run time' );
			}
			return { code: `(()=>{${ code }\n})();\n`, map: null };
		}
	};
}

export default defineConfig( {
	plugins: [ vue(), bundledPackages(), classicHeadScript() ],
	base: '/ui/assets/',
	build: {
		outDir: 'dist',
		emptyOutDir: true,
		manifest: 'manifest.json',
		assetsDir: '',
		cssCodeSplit: true,
		rollupOptions: {
			input: {
				site: 'src/site.css',
				'site-rtl': 'src/site-rtl.css',
				main: 'src/main.js',
				// The blocking head script (ADR 0085 §6): no imports of its own at run
				// time, so it is one file a classic <script> can load.
				head: 'src/head.js',
				// The typefaces of each shipped type set (ADR 0086 §3), whose
				// @font-face rules triplespace-ui writes into the type set's stylesheet.
				'typeset-default26': 'src/typesets/default26.css',
				'typeset-hyperlegible': 'src/typesets/hyperlegible.css',
				'typeset-opendyslexic': 'src/typesets/opendyslexic.css',
				'typeset-retro': 'src/typesets/retro.css',
				'typeset-nordic': 'src/typesets/nordic.css',
				'typeset-cozy': 'src/typesets/cozy.css',
				'typeset-mainframe': 'src/typesets/mainframe.css',
				'typeset-drafting': 'src/typesets/drafting.css',
				// The typefaces of each fallback layer (ADR 0093 §3), written by
				// scripts/fallbacks.js and written by triplespace-ui into the stylesheet of
				// every type set that names the layer.
				'fallback-noto': 'src/fallbacks/noto.css'
			}
		}
	},
	test: {
		// Vitest's unit tests live beside the code; e2e/ is Playwright's (npm run e2e).
		include: [ 'src/**/*.{test,spec}.{js,ts}' ],
		environment: 'happy-dom'
	}
} );
