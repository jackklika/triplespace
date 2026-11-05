// Keyboard previous and next on a diff page (ADR 0010 §6): the left and right arrow keys
// follow the page's rel="prev" and rel="next" links, as the "diff arrow keys" gadget does on
// Wikimedia wikis; reversed on a right-to-left page, where the older side is on the right.
// A key with a modifier, or pressed in a field, is left alone.

/**
 * Whether a key press belongs to a field the reader is typing in.
 *
 * @param {EventTarget|null} t
 * @return {boolean}
 */
function typing( t ) {
	if ( !( t instanceof Element ) ) {
		return false;
	}
	return t.matches( 'input, textarea, select' ) || /** @type {HTMLElement} */ ( t ).isContentEditable;
}

/**
 * Wires the arrow keys on a page that has a diff, marked `data-ts-diffnav`.
 *
 * @param {Document} doc
 * @param {{ location: { assign: function(string): void } }} win
 */
export function initDiffNav( doc, win ) {
	const root = doc.querySelector( '[data-ts-diffnav]' );
	if ( !root ) {
		return;
	}
	doc.addEventListener( 'keydown', ( e ) => {
		const modified = e.altKey || e.ctrlKey || e.metaKey || e.shiftKey;
		if ( e.defaultPrevented || modified || typing( e.target ) ) {
			return;
		}
		let rel;
		if ( e.key === 'ArrowLeft' ) {
			rel = 'prev';
		} else if ( e.key === 'ArrowRight' ) {
			rel = 'next';
		} else {
			return;
		}
		if ( doc.documentElement.dir === 'rtl' ) {
			rel = rel === 'prev' ? 'next' : 'prev';
		}
		const link = /** @type {HTMLAnchorElement|null} */ ( root.querySelector( `a[rel~="${ rel }"]` ) );
		if ( !link ) {
			return;
		}
		e.preventDefault();
		win.location.assign( link.href );
	} );
}
