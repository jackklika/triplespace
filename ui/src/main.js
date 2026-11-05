// The site's script entry (ADR 0034 §4). Pages read and work without it; it marks the
// document as having JavaScript (the head script has usually done so already), wires the
// Appearance menu (ADR 0085 §6), the challenge of a registration or login form
// (ADR 0087 §5) and a diff page's arrow keys (ADR 0010 §6), and loads the editing
// components only on a page that has something the viewer may edit (the server writes an
// edit data block there), so a reader downloads none of Vue.
import { initAppearance } from './appearance/menu.js';
import { initChallenges } from './challenge/form.js';
import { initDiffNav } from './diffnav/diffnav.js';

document.documentElement.classList.replace( 'client-nojs', 'client-js' );
initAppearance( document, window );
initChallenges( document );
initDiffNav( document, window );
if ( document.getElementById( 'ts-edit-data' ) ) {
	import( './edit/index.ts' );
}
