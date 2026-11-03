// Everything the site draws takes its colours from tokens, so every palette variant
// reaches it (ADR 0086 §2): the site's stylesheets, components and the shipped skins'
// templates and stylesheets hold no colour literal, except as the fallback of a `var()`, which
// applies only where no palette does.
import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname( fileURLToPath( import.meta.url ) );
const roots = [
	[ here, /\.(css|vue)$/ ],
	[ join( here, '../../skins' ), /\.(html|css)$/ ]
];
const COLOUR = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?|oklch|oklab|lab|lch)\(/g;
const FALLBACK = /var\(\s*--[a-z0-9-]+\s*,\s*[^()]*(?:\([^()]*\))?[^()]*\)/g;

/**
 * Every file under a directory whose name matches.
 *
 * @param {string} dir
 * @param {RegExp} pattern
 * @return {string[]}
 */
function files( dir, pattern ) {
	// eslint-disable-next-line security/detect-non-literal-fs-filename
	return readdirSync( dir, { withFileTypes: true, recursive: true } )
		.filter( ( e ) => e.isFile() && pattern.test( e.name ) )
		.map( ( e ) => join( e.parentPath, e.name ) );
}

describe( 'colours', () => {
	for ( const [ dir, pattern ] of roots ) {
		for ( const file of files( dir, pattern ) ) {
			it( `${ file.slice( here.length - 3 ) } names its colours by token`, () => {
				// eslint-disable-next-line security/detect-non-literal-fs-filename
				const text = readFileSync( file, 'utf8' )
					.replace( /\/\*[\s\S]*?\*\//g, '' )
					.replace( FALLBACK, '' );
				expect( text.match( COLOUR ) || [] ).toEqual( [] );
			} );
		}
	}
} );
