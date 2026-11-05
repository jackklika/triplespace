// A document page's history and diff (ADR 0010 §5.6, §6; chapter 19 §3.7–3.8): the
// history's compare form, and the diff page's views, context, permalink and arrow keys.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

const TITLE = 'Project:Diff_views';

// Three revisions: the second changes a sentence and the last line, the third the
// first list line, so that the default context gives two hunks between the first two.
const REVISIONS = [
	"== History ==\nThe '''library''' opened in ''1901''.\n\n* Line a\n* Line b\n* Line c\n* Line d\n* Line e\n* Line f\n* Line g",
	"== History ==\nThe '''library''' opened in ''1902''.\n\n* Line a\n* Line b\n* Line c\n* Line d\n* Line e\n* Line f\n* Line G",
	"== History ==\nThe '''library''' opened in ''1902''.\n\n* Line A\n* Line b\n* Line c\n* Line d\n* Line e\n* Line f\n* Line G"
];

/**
 * Logs in as the owner, returning to `back`.
 *
 * @param {Object} page A Playwright page
 * @param {string} back
 */
async function logIn( page, back ) {
	await page.goto( `/wiki/Special:UserLogin?returnto=${ back }` );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
}

/**
 * The axe violations on a page. axe runs as a script, so the page is opened in a context
 * that allows scripts.
 *
 * @param {Object} browser A Playwright browser
 * @param {string} path
 * @return {Promise<string[]>} The violations, as `id: targets`
 */
async function violations( browser, path ) {
	const context = await browser.newContext( { javaScriptEnabled: true } );
	const page = await context.newPage();
	await page.goto( path );
	const results = await new AxeBuilder( { page } )
		.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
		.analyze();
	await context.close();
	return results.violations.map( ( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target.join( ' ' ) ).join( ', ' ) }` );
}

/**
 * Waits until the page's address has these query parameters.
 *
 * @param {Object} page A Playwright page
 * @param {Object<string, string>} want
 */
async function toHaveQuery( page, want ) {
	await page.waitForURL( ( url ) => Object.entries( want )
		.every( ( [ k, v ] ) => url.searchParams.get( k ) === v ) );
}

/**
 * The page's revision IDs, newest first, from its history's compare form.
 *
 * @param {Object} page A Playwright page
 * @return {Promise<string[]>}
 */
async function revids( page ) {
	await page.goto( `/wiki/${ TITLE }?action=history` );
	return page.locator( 'input[name="diff"]' ).evaluateAll( ( els ) => els.map( ( e ) => e.value ) );
}

test.describe.serial( 'a page with three revisions', () => {
	test( 'is written through the edit form', async ( { page } ) => {
		await logIn( page, 'Project:Home' );
		for ( const text of REVISIONS ) {
			await page.goto( `/w/index.php?title=${ TITLE }&action=edit` );
			await page.locator( '#wpTextbox1' ).fill( text );
			await page.locator( 'button[name="wpSave"]' ).click();
			await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'librarybase:Diff views' );
		}
	} );

	test( 'the history compares any two revisions without JavaScript', async ( { page } ) => {
		const ids = await revids( page );
		expect( ids ).toHaveLength( 3 );
		const form = page.locator( 'form.ts-history__compare' );
		await expect( form.getByRole( 'columnheader', { name: 'Compare' } ) ).toBeVisible();
		// The newest two are chosen to begin with, and the newest cannot be the older side.
		await expect( form.locator( `input[name="diff"][value="${ ids[ 0 ] }"]` ) ).toBeChecked();
		await expect( form.locator( `input[name="oldid"][value="${ ids[ 1 ] }"]` ) ).toBeChecked();
		await expect( form.locator( `input[name="oldid"][value="${ ids[ 0 ] }"]` ) ).toHaveCount( 0 );
		// The oldest against the newest.
		await form.locator( `input[name="oldid"][value="${ ids[ 2 ] }"]` ).check();
		await form.getByRole( 'button', { name: 'Compare selected revisions' } ).click();
		await toHaveQuery( page, { oldid: ids[ 2 ], diff: ids[ 0 ] } );
		await expect( page.locator( '.ts-page-diff__table .diff-addedline' ) ).toHaveCount( 3 );
		// Chosen the other way round, the older is still on the left, as in MediaWiki.
		await page.goto( `/w/index.php?title=${ TITLE }&oldid=${ ids[ 0 ] }&diff=${ ids[ 2 ] }` );
		await expect( page.locator( '.diff-otitle' ) ).toContainText( 'Revision as of' );
		await expect( page.locator( '.diff-ntitle' ) ).toContainText( 'Latest revision as of' );
	} );

	test( 'the diff page offers its views, more context and a permalink', async ( { page } ) => {
		const [ , second, first ] = await revids( page );
		await page.goto( `/wiki/${ TITLE }?diff=${ second }&oldid=${ first }` );
		const views = page.getByRole( 'navigation', { name: 'Diff views' } );
		await expect( views.getByRole( 'link', { name: 'Source' } ) ).toHaveAttribute( 'aria-current', 'page' );
		// The source diff: two hunks, two lines of context each.
		await expect( page.locator( '.ts-page-diff__table tr:has(.diff-lineno)' ) ).toHaveCount( 2 );
		await expect( page.locator( '#ts-diff-permalink' ) ).toHaveValue( `[[Special:Diff/${ first }/${ second }]]` );

		// More context joins them, and keeps the view.
		await page.getByRole( 'link', { name: 'More context' } ).click();
		await expect( page ).toHaveURL( /&context=10$/ );
		await expect( page.locator( '.ts-page-diff__table tr:has(.diff-lineno)' ) ).toHaveCount( 1 );
		await expect( page.locator( '.ts-page-diff__table .diff-context' ).first() ).toBeVisible();

		// Text only: the prose, without the markup.
		await views.getByRole( 'link', { name: 'Text only' } ).click();
		await expect( page ).toHaveURL( /&view=text&context=10$/ );
		const table = page.locator( '.ts-page-diff__table' );
		await expect( table.locator( '.diff-addedline' ).first() ).toContainText( 'The library opened in 1902.' );
		await expect( table ).not.toContainText( "''" );

		// Side by side: both revisions rendered.
		await views.getByRole( 'link', { name: 'Side by side' } ).click();
		await expect( page ).toHaveURL( /&view=rendered$/ );
		const columns = page.locator( '.ts-page-diff__column' );
		await expect( columns ).toHaveCount( 2 );
		await expect( columns.nth( 0 ).locator( 'b' ) ).toHaveText( 'library' );
		await expect( columns.nth( 0 ) ).toContainText( '1901' );
		await expect( columns.nth( 1 ) ).toContainText( '1902' );
		await expect( page.getByRole( 'link', { name: 'More context' } ) ).toHaveCount( 0 );

		// The permalink reads back.
		await page.goto( `/wiki/Special:Diff/${ first }/${ second }` );
		await expect( page.locator( '.ts-page-diff__table tr:has(.diff-lineno)' ) ).toHaveCount( 2 );
	} );

	test( 'the history and every view of the diff pass axe', async ( { page, browser } ) => {
		const [ , second, first ] = await revids( page );
		const diff = `/wiki/${ TITLE }?diff=${ second }&oldid=${ first }`;
		for ( const path of [
			`/wiki/${ TITLE }?action=history`,
			diff,
			`${ diff }&context=all`,
			`${ diff }&view=text`,
			`${ diff }&view=rendered`
		] ) {
			expect( await violations( browser, path ), path ).toEqual( [] );
		}
	} );

	test.describe( 'with JavaScript', () => {
		test.use( { javaScriptEnabled: true } );

		test( 'the arrow keys step to the older and newer diff', async ( { page } ) => {
			const [ third, second ] = await revids( page );
			// The second revision against the first; its newer side has a next diff.
			await page.goto( `/wiki/${ TITLE }?diff=${ second }&view=text` );
			await expect( page.locator( '.diff-ntitle a[rel="next"]' ) ).toBeVisible();
			await page.keyboard.press( 'ArrowRight' );
			await toHaveQuery( page, { diff: 'next', oldid: second, view: 'text' } );
			await expect( page.locator( '.diff-ntitle' ) ).toContainText( 'Latest revision as of' );
			// The third against the second; its older side has a previous diff.
			await page.goto( `/wiki/${ TITLE }?diff=${ third }` );
			await page.keyboard.press( 'ArrowLeft' );
			await toHaveQuery( page, { diff: 'prev', oldid: second } );
			// Typing in the permalink field does not leave the page.
			await page.locator( '#ts-diff-permalink' ).focus();
			const here = page.url();
			await page.keyboard.press( 'ArrowRight' );
			expect( page.url() ).toBe( here );
		} );
	} );
} );
