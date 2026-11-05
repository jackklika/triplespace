/**
 * A stand-in for internetdomains.wiki's `api.php`, for `triplespace follow --once` in the
 * end-to-end suite. A load (`triplespace sync`) writes no activity rows for its records
 * (ADR 0013 §5.5; chapter 03 §4.6); the follower's records are `sync` rows, which the
 * history tab and recent changes show. This wiki has one recent change: an upstream edit of
 * Q11 (wikipedia.org) at revision 57 that adds a German description, so the record differs
 * from the load's and nothing the English pages show changes.
 *
 * It answers the four requests the follower makes (ADR 0092 §1–2): `siteinfo` namespaces;
 * the oldest recent change, which must be older than the load's position (a day before the
 * fixture's `modified`); the recent changes since that position; `wbgetentities`.
 *
 * node wiki.mjs <listen port> <internetdomains.json>
 */
import http from 'node:http';
import { readFileSync } from 'node:fs';

const [ port, fixture ] = process.argv.slice( 2 );
// The file is the server's test fixture, named by run.sh.
// eslint-disable-next-line security/detect-non-literal-fs-filename
const entities = JSON.parse( readFileSync( fixture, 'utf8' ) );
const q11 = entities.find( ( e ) => e.id === 'Q11' );
const edited = {
	...q11,
	lastrevid: 57,
	modified: '2026-10-02T00:00:00Z',
	descriptions: {
		...q11.descriptions,
		de: { language: 'de', value: 'Freie Online-Enzyklopädie' }
	}
};

function answer( params ) {
	if ( params.get( 'meta' ) === 'siteinfo' ) {
		return { batchcomplete: true, query: { namespaces: {
			120: { id: 120, canonical: 'Item', defaultcontentmodel: 'wikibase-item' },
			122: { id: 122, canonical: 'Property', defaultcontentmodel: 'wikibase-property' }
		} } };
	}
	if ( params.get( 'list' ) === 'recentchanges' ) {
		// The oldest-change probe asks for timestamps only, one row.
		if ( params.get( 'rcprop' ) === 'timestamp' ) {
			return { batchcomplete: true, query: { recentchanges: [
				{ type: 'edit', ns: 120, title: 'Item:Q9', timestamp: '2026-09-01T00:00:00Z' }
			] } };
		}
		return { batchcomplete: true, query: { recentchanges: [
			{ rcid: 1001, type: 'edit', ns: 120, title: 'Item:Q11', revid: 57,
				timestamp: '2026-10-02T00:00:00Z' }
		] } };
	}
	if ( params.get( 'action' ) === 'wbgetentities' ) {
		const found = {};
		for ( const id of ( params.get( 'ids' ) || '' ).split( '|' ) ) {
			const e = id === 'Q11' ? edited : entities.find( ( x ) => x.id === id );
			found[ id ] = e || { id, missing: '' };
		}
		return { entities: found, success: 1 };
	}
	return { error: { code: 'badvalue', info: `not served by the e2e wiki: ${ params }` } };
}

http.createServer( ( req, res ) => {
	const params = new URL( req.url, 'http://wiki' ).searchParams;
	res.writeHead( 200, { 'Content-Type': 'application/json; charset=utf-8' } );
	res.end( JSON.stringify( answer( params ) ) );
} ).listen( Number( port ), '127.0.0.1' );
