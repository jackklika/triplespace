// Notifications (ADR 0021 §6; chapter 19 §6.2): a new account starts a topic on Alice's
// user talk page; Alice, with JavaScript off, sees the orange bar and the bell's count,
// finds one notification on Special:Notifications (a talk message, though it also
// mentions her), loses the bell's mark by looking and the orange bar by reading her talk
// page, and marks everything read. Then axe on Special:Notifications.
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

test.describe.configure( { mode: 'serial' } );

test( 'a new account starts a topic on a user talk page', async ( { browser } ) => {
	// Registration needs the browser to solve the challenge.
	const context = await browser.newContext( { javaScriptEnabled: true } );
	const page = await context.newPage();
	await page.goto( '/wiki/Special:CreateAccount?returnto=User_talk:Alice' );
	await page.getByLabel( 'Username' ).fill( 'Talk Writer' );
	await page.getByLabel( 'Password', { exact: true } ).fill( 'talk-writer-password-1' );
	await page.getByLabel( 'Confirm password' ).fill( 'talk-writer-password-1' );
	await page.getByRole( 'button', { name: 'Create your account' } ).click();
	await page.waitForURL( '**/wiki/User_talk:Alice', { timeout: 60000 } );
	await page.getByRole( 'link', { name: 'Add topic' } ).click();
	await page.getByLabel( 'Subject' ).fill( 'A word, Alice' );
	await page.getByLabel( 'Description' ).fill( 'Hello [[User:Alice]], about your edits.' );
	await page.getByRole( 'button', { name: 'Add topic' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/User_talk:Alice#thread-\d+$/ );
	await expect( page.locator( '.ts-thread' ).filter( { hasText: 'A word, Alice' } ) ).toBeVisible();
	await context.close();
} );

test( 'Alice sees the orange bar and the bell, and reads her messages', async ( { page } ) => {
	await logIn( page, 'Main Page' );
	const bar = page.locator( '.ts-orange-bar' );
	await expect( bar ).toContainText( 'You have a new message on your talk page.' );
	const bell = page.locator( 'a.ts-bell' );
	await expect( bell ).toHaveAttribute( 'aria-label', /^Notifications, \d+ unread$/ );
	await expect( bell ).toHaveClass( /ts-bell--unseen/ );

	await bell.click();
	await expect( page ).toHaveURL( /\/wiki\/Special:Notifications$/ );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Notifications' );
	const items = page.locator( '.ts-notif' ).filter( { hasText: 'A word, Alice' } );
	// A talk message, though it also mentions her: one notification per post.
	await expect( items ).toHaveCount( 1 );
	await expect( items ).toContainText( 'Talk Writer posted in “A word, Alice” on your talk page.' );
	await expect( items ).toHaveClass( /ts-notif--unread/ );
	// Looking is seeing: the bell's mark goes, the count stays.
	await expect( page.locator( 'a.ts-bell' ) ).not.toHaveClass( /ts-bell--unseen/ );
	await expect( page.locator( '.ts-bell__count' ) ).toBeVisible();

	// Reading her talk page reads its messages: the orange bar goes.
	await page.locator( '.ts-orange-bar' ).getByRole( 'link', { name: 'your talk page' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/User_talk:Alice$/ );
	await expect( page.locator( '.ts-orange-bar' ) ).toHaveCount( 0 );
	await page.goto( '/wiki/Main_Page' );
	await expect( page.locator( '.ts-orange-bar' ) ).toHaveCount( 0 );

	await page.goto( '/wiki/Special:Notifications' );
	const item = page.locator( '.ts-notif' ).filter( { hasText: 'A word, Alice' } );
	await expect( item ).not.toHaveClass( /ts-notif--unread/ );
	await item.getByRole( 'link', { name: '“A word, Alice”' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Thread:.*#post-\d+$/ );

	await page.goto( '/wiki/Special:Notifications' );
	const markAll = page.getByRole( 'button', { name: 'Mark all as read' } );
	if ( await markAll.count() ) {
		await markAll.click();
	}
	await expect( page ).toHaveURL( /\/wiki\/Special:Notifications$/ );
	await expect( page.locator( '.ts-notif--unread' ) ).toHaveCount( 0 );
	await expect( page.locator( '.ts-bell__count' ) ).toHaveCount( 0 );
	await expect( page.locator( 'a.ts-bell' ) ).toHaveAttribute( 'aria-label', 'Notifications' );
} );

test( 'Special:Notifications passes axe', async ( { browser } ) => {
	const context = await browser.newContext( { javaScriptEnabled: true } );
	const page = await context.newPage();
	await logIn( page, 'Special:Notifications' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Notifications' );
	const results = await new AxeBuilder( { page } )
		.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
		.analyze();
	expect( results.violations.map( ( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }` ) ).toEqual( [] );
	await context.close();
} );
