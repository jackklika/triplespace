import { describe, it, expect, vi } from 'vitest';
import { initDiffNav } from './diffnav.js';

// ADR 0010 §6: the arrow keys step a diff page to the older and newer diff.
describe( 'initDiffNav', () => {
	/** @type {{ location: { assign: Function } }} */
	let win;

	/**
	 * A page in a document of its own, so each test's listener is its own.
	 *
	 * @param {string} body
	 * @param {string} [dir]
	 * @return {Document}
	 */
	function page( body, dir = 'ltr' ) {
		win = { location: { assign: vi.fn() } };
		const doc = document.implementation.createHTMLDocument( 't' );
		doc.documentElement.dir = dir;
		doc.body.innerHTML = body;
		initDiffNav( doc, win );
		return doc;
	}

	/**
	 * @param {Document} doc
	 * @param {string} key
	 * @param {Object} [extra]
	 * @param {EventTarget} [target]
	 */
	function press( doc, key, extra = {}, target = doc.body ) {
		target.dispatchEvent( new KeyboardEvent( 'keydown', Object.assign( { key, bubbles: true, cancelable: true }, extra ) ) );
	}

	const DIFF = '<div data-ts-diffnav><a href="/wiki/X?diff=prev&amp;oldid=4" rel="prev">Older</a><a href="/wiki/X?diff=next&amp;oldid=5" rel="next">Newer</a><input id="f"></div>';

	it( 'follows rel=prev on the left arrow and rel=next on the right', () => {
		const doc = page( DIFF );
		press( doc, 'ArrowLeft' );
		expect( win.location.assign ).toHaveBeenLastCalledWith( expect.stringContaining( 'diff=prev&oldid=4' ) );
		press( doc, 'ArrowRight' );
		expect( win.location.assign ).toHaveBeenLastCalledWith( expect.stringContaining( 'diff=next&oldid=5' ) );
	} );

	it( 'reverses the arrows on a right-to-left page', () => {
		const doc = page( DIFF, 'rtl' );
		press( doc, 'ArrowLeft' );
		expect( win.location.assign ).toHaveBeenLastCalledWith( expect.stringContaining( 'diff=next' ) );
	} );

	it( 'leaves modified keys, other keys and typing alone', () => {
		const doc = page( DIFF );
		press( doc, 'ArrowLeft', { altKey: true } );
		press( doc, 'ArrowRight', { ctrlKey: true } );
		press( doc, 'ArrowUp' );
		press( doc, 'ArrowLeft', {}, /** @type {HTMLElement} */ ( doc.getElementById( 'f' ) ) );
		expect( win.location.assign ).not.toHaveBeenCalled();
	} );

	it( 'does nothing where there is no link that way, or no diff', () => {
		let doc = page( '<div data-ts-diffnav><a href="/wiki/X?diff=4" rel="prev">Older</a></div>' );
		press( doc, 'ArrowRight' );
		expect( win.location.assign ).not.toHaveBeenCalled();
		doc = page( '<a href="/x" rel="prev">Older</a>' );
		press( doc, 'ArrowLeft' );
		expect( win.location.assign ).not.toHaveBeenCalled();
	} );
} );
