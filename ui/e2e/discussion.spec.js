// Talk pages and threads (ADR 0019 §8; chapter 19 §6.1), with JavaScript off: starting a
// thread, replying with a status, and the signature an editor types by habit removed
// without comment (chapter 14 §1.6); a thread's actions from the page (14 §1.2, §2.1,
// §3.6, §4): pin, list on another page, move, rename, close and reopen; then axe.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

/**
 * Logs Alice in through the site's own form.
 *
 * @param {Object} page A Playwright page
 * @param {string} returnto The title to come back to
 */
async function logIn( page, returnto ) {
	await page.goto( `/wiki/Special:UserLogin?returnto=${ encodeURIComponent( returnto ) }` );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
}

/**
 * axe's WCAG 2.1 AA violations on the page as it stands.
 *
 * @param {Object} page A Playwright page
 * @return {Promise<string[]>}
 */
async function violations( page ) {
	const results = await new AxeBuilder( { page } )
		.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
		.analyze();
	return results.violations.map(
		( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
	);
}

test.describe.configure( { mode: 'serial' } );

test( 'an empty talk page reads without JavaScript and offers no form to a reader', async ( { page } ) => {
	const response = await page.goto( '/wiki/Item_talk:Q5' );
	expect( response.status() ).toBe( 200 );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Item talk:Q5' );
	await expect( page.getByText( 'There are no discussions on this page yet.' ) ).toBeVisible();
	await expect( page.getByRole( 'link', { name: 'Discussion' } ) ).toHaveAttribute( 'aria-current', 'page' );
	await expect( page.getByRole( 'link', { name: 'Edit', exact: true } ) ).toHaveCount( 0 );
	await expect( page.getByRole( 'link', { name: 'Add topic' } ) ).toHaveCount( 0 );
} );

test( 'a thread is started, replied to and resolved from the site', async ( { page } ) => {
	await logIn( page, 'Item talk:Q5' );
	await page.getByRole( 'link', { name: 'Add topic' } ).click();
	await page.getByLabel( 'Subject' ).fill( 'Is five a city?' );
	await page.getByLabel( 'Description' ).fill( 'The label says so, the sources do not. --~~~~' );
	await expect( page.getByText( 'Posts on this wiki use Markdown.' ) ).toBeVisible();
	await page.getByRole( 'button', { name: 'Add topic' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item_talk:Q5#thread-\d+$/ );
	const thread = page.locator( '.ts-thread' ).filter( { hasText: 'Is five a city?' } );
	await expect( thread.locator( '.ts-post' ).first() ).toContainText( 'The label says so, the sources do not.' );
	await expect( thread ).not.toContainText( '~~~~' );
	// No status yet: the thread reads as an ordinary section.
	await expect( thread.locator( '.ts-status' ) ).toHaveCount( 0 );

	await thread.getByRole( 'link', { name: 'Reply' } ).first().click();
	await expect( page.getByLabel( 'Reply to Alice' ) ).toBeVisible();
	await page.getByLabel( 'Reply to Alice' ).fill( 'Checked: it is not. ~~~~' );
	await page.getByLabel( 'Status' ).selectOption( 'resolved' );
	await page.getByRole( 'button', { name: 'Reply' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Thread:.*#post-\d+$/ );
	await expect( page.locator( '.ts-thread-page__state .ts-status' ) ).toHaveText( 'Resolved' );
	await expect( page.locator( '.ts-post' ).nth( 1 ) ).toContainText( 'Checked: it is not.' );
	await expect( page.locator( '.ts-post' ).nth( 1 ) ).not.toContainText( '~~~~' );

	// A post that is only a signature is refused, and nothing is lost.
	await page.getByLabel( 'Reply to this thread' ).fill( '~~~~' );
	await page.getByRole( 'button', { name: 'Reply' } ).click();
	await expect( page.getByText( 'Your post has no text.' ) ).toBeVisible();

	// Back on the talk page, the resolved thread is collapsed behind its summary.
	await page.getByRole( 'link', { name: 'Item talk:Q5' } ).click();
	const closed = page.locator( '.ts-thread' ).filter( { hasText: 'Is five a city?' } );
	await expect( closed.locator( '.ts-thread__head .ts-status' ) ).toHaveText( 'Resolved' );
	await expect( closed.getByText( 'Checked: it is not.' ) ).toBeHidden();
	await closed.locator( 'summary' ).click();
	await expect( closed.getByText( 'Checked: it is not.' ) ).toBeVisible();
} );

test( 'the talk page and the thread page pass axe', async ( { browser } ) => {
	const context = await browser.newContext( { javaScriptEnabled: true } );
	const page = await context.newPage();
	await logIn( page, 'Item talk:Q5' );
	await page.goto( '/wiki/Item_talk:Q5?action=addtopic' );
	expect( await violations( page ) ).toEqual( [] );
	await page.locator( '.ts-thread__subject a' ).first().click();
	await expect( page.locator( 'form#reply' ) ).toBeVisible();
	expect( await violations( page ) ).toEqual( [] );
	await context.close();
} );

test( 'the talk page\'s history lists its threads\' rows without JavaScript, and passes axe', async ( { browser, page } ) => {
	await page.goto( '/wiki/Item_talk:Q5' );
	await page.getByRole( 'link', { name: 'History' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item_talk:Q5\?action=history$/ );
	await expect( page.getByRole( 'link', { name: 'History' } ) ).toHaveAttribute( 'aria-current', 'page' );
	const rows = page.locator( '.ts-change' );
	await expect( rows.filter( { hasText: 'started the thread' } ) ).toHaveCount( 1 );
	await expect( rows.filter( { hasText: 'replied' } ).first() ).toContainText( 'Is five a city?' );
	// A row's time goes to the post it wrote.
	const href = await rows.filter( { hasText: 'started the thread' } ).locator( 'a.ts-change__diff' ).getAttribute( 'href' );
	expect( href ).toMatch( /^\/wiki\/Thread:.*#post-\d+$/ );

	const context = await browser.newContext( { javaScriptEnabled: true } );
	const p = await context.newPage();
	await p.goto( '/wiki/Item_talk:Q5?action=history' );
	expect( await violations( p ) ).toEqual( [] );
	await context.close();
} );

/**
 * Opens one of the thread page's actions from its overflow menu.
 *
 * @param {Object} page A Playwright page
 * @param {string} name The menu item
 */
async function action( page, name ) {
	await page.locator( 'details.ts-tabs__more > summary' ).click();
	await page.locator( 'details.ts-tabs__more' ).getByRole( 'link', { name } ).click();
}

test( 'a thread is pinned, listed on another page, moved, renamed, closed and reopened from the page', async ( { page } ) => {
	await logIn( page, 'Item talk:Q6' );
	await page.goto( '/wiki/Item_talk:Q6?action=addtopic' );
	await page.getByLabel( 'Subject' ).fill( 'Six or more?' );
	await page.getByLabel( 'Description' ).fill( 'Which is it?' );
	await page.getByRole( 'button', { name: 'Add topic' } ).click();
	const listed = page.locator( '.ts-thread' ).filter( { hasText: 'Six or more?' } );

	// Pin here, from the talk page: the form opens with this page chosen.
	await listed.getByRole( 'link', { name: 'Pin here' } ).click();
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Pin a thread' );
	await expect( page.getByLabel( 'Page' ) ).toHaveValue( 'Item talk:Q6' );
	await page.getByRole( 'button', { name: 'Pin' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item_talk:Q6#thread-\d+$/ );
	await expect( listed.locator( '.ts-thread__pin' ) ).toHaveCount( 1 );
	await listed.getByRole( 'link', { name: 'Unpin here' } ).click();
	await page.getByRole( 'button', { name: 'Unpin' } ).click();
	await expect( listed.locator( '.ts-thread__pin' ) ).toHaveCount( 0 );

	// List on another page, by its subject's title.
	await listed.locator( '.ts-thread__subject a' ).click();
	await action( page, 'List on…' );
	await page.getByLabel( 'List it on' ).fill( 'Item:Q5' );
	await page.getByRole( 'button', { name: 'List it there' } ).click();
	await expect( page.locator( '.ts-thread-page__where' ) ).toContainText( 'Item talk:Q5' );
	await page.goto( '/wiki/Item_talk:Q5' );
	await expect( page.locator( '.ts-thread' ).filter( { hasText: 'Six or more?' } ) ).toHaveCount( 1 );
	await page.locator( '.ts-thread' ).filter( { hasText: 'Six or more?' } ).locator( '.ts-thread__subject a' ).click();

	// A page that cannot hold a thread is refused on the form, with the text kept.
	await action( page, 'Move…' );
	await page.getByLabel( 'New home' ).fill( 'Thread:Nowhere' );
	await page.getByRole( 'button', { name: 'Move' } ).click();
	await expect( page.getByText( '"Thread:Nowhere" is not a page a thread can be on.' ) ).toBeVisible();
	await expect( page.getByLabel( 'New home' ) ).toHaveValue( 'Thread:Nowhere' );

	// Move it home to the page it is listed on, keeping the old home as a listing.
	await page.getByLabel( 'New home' ).fill( 'Item talk:Q5' );
	await page.getByLabel( 'Keep it listed on its old home' ).check();
	await page.getByRole( 'button', { name: 'Move' } ).click();
	const where = page.locator( '.ts-thread-page__where' );
	await expect( where.locator( 'div' ).filter( { hasText: 'Home' } ) ).toContainText( 'Item talk:Q5' );
	await expect( where.locator( 'div' ).filter( { hasText: 'Also on' } ) ).toContainText( 'Item talk:Q6' );

	// Rename: the title follows the subject.
	await action( page, 'Rename…' );
	await page.getByLabel( 'Subject' ).fill( 'Six, or more' );
	await page.getByRole( 'button', { name: 'Rename' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Thread:.*Six,_or_more$/ );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toContainText( 'Six, or more' );

	// Close with a status and no statement, then reopen.
	await action( page, 'Close…' );
	await page.getByLabel( 'Status' ).selectOption( 'wontfix' );
	await page.getByRole( 'button', { name: 'Close' } ).click();
	await expect( page.locator( '.ts-thread-page__state .ts-status' ) ).toHaveText( "Won't fix" );
	await action( page, 'Reopen…' );
	await page.getByRole( 'button', { name: 'Reopen' } ).click();
	await expect( page.locator( '.ts-thread-page__state .ts-status' ) ).toHaveText( 'Open' );

	// Remove the listing on the old home.
	await action( page, 'Remove a listing…' );
	await page.getByRole( 'button', { name: 'Remove the listing' } ).click();
	await expect( page.locator( '.ts-thread-page__where' ) ).not.toContainText( 'Item talk:Q6' );
} );

test( 'a thread action\'s form passes axe, and a reader is sent to log in', async ( { browser, page } ) => {
	await page.goto( '/wiki/Item_talk:Q5' );
	const href = await page.locator( '.ts-thread__subject a' ).first().getAttribute( 'href' );
	await expect( page.getByRole( 'link', { name: 'Pin here' } ) ).toHaveCount( 0 );
	await page.goto( `${ href }?action=thread&op=rename` );
	await expect( page ).toHaveURL( /Special:UserLogin/ );

	const context = await browser.newContext( { javaScriptEnabled: true } );
	const p = await context.newPage();
	await logIn( p, 'Item talk:Q5' );
	await p.goto( `${ href }?action=thread&op=move` );
	await expect( p.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Move a thread' );
	expect( await violations( p ) ).toEqual( [] );
	await context.close();
} );
