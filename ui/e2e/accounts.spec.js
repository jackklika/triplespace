// The account pages' POSTed states, without JavaScript (ADR 0010 §10, 0085 §5; chapter 19
// §2.6): what a refused login, logging out and saving preferences answer. The skin
// migration's parity corpus records GET requests only, so these are the states that the
// suite under TRIPLESPACE_WEB_RENDERER=compare checks for group 9 (19 §5.10): a refused
// login keeps the name and says why, a viewer already logged in is told so, logging out
// asks and then does, a refused preference comes back with the reader's own text and the
// reason, and a good one is saved and then reset.
import { test, expect } from '@playwright/test';

/**
 * Logs Alice in through the site's own form and waits to be returned.
 *
 * @param {Object} page A Playwright page
 * @param {string} back The title to return to
 */
async function logIn( page, back ) {
	await page.goto( `/wiki/Special:UserLogin?returnto=${ back }` );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
	await page.waitForURL( `**/wiki/${ back }` );
}

test( 'a refused login says why and keeps the name and the way back', async ( { page } ) => {
	await page.goto( '/wiki/Special:UserLogin?returnto=Special:RecentChanges' );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( 'not-the-password' );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
	await expect( page.locator( '.ts-account-form .cdx-message--error' ) )
		.toContainText( 'Incorrect username or password' );
	await expect( page.getByLabel( 'Username' ) ).toHaveValue( 'Alice' );
	await expect( page.getByLabel( 'Password' ) ).toHaveValue( '' );
	await expect( page.locator( 'form[action*="Special:UserLogin"] input[name="returnto"]' ) ).toHaveValue( 'Special:RecentChanges' );
} );

test( 'a viewer already logged in is told so, and logging out asks and then does', async ( { page } ) => {
	await logIn( page, 'Special:RecentChanges' );
	await page.goto( '/wiki/Special:UserLogin' );
	await expect( page.locator( '.ts-account-form' ) ).toContainText( 'You are logged in as Alice.' );
	await expect( page.locator( '.ts-account-form form' ) ).toHaveCount( 0 );
	await page.goto( '/wiki/Special:UserLogout?returnto=Special:RecentChanges' );
	await expect( page.locator( '.ts-account-form' ) ).toContainText( 'Do you want to log out?' );
	await page.getByRole( 'button', { name: 'Log out', exact: true } ).click();
	await page.waitForURL( '**/wiki/Special:RecentChanges' );
	await page.goto( '/wiki/Special:UserLogout' );
	await expect( page.locator( '.ts-account-form' ) ).toContainText( 'You are not logged in.' );
	await expect( page.locator( '.ts-account-form form' ) ).toHaveCount( 0 );
} );

test( 'a refused preference comes back with the reader\'s text and why; a good one saves and resets', async ( { page } ) => {
	await logIn( page, 'Special:Preferences' );
	// Refused: the reader's own text comes back in the field, with the reason on its row.
	const pins = page.locator( '[id="ts-p-shapes.pins"]' );
	await pins.fill( '{' );
	await page.locator( '#statements' ).getByRole( 'button', { name: 'Save', exact: true } ).click();
	await expect( page.locator( '[id="pref-shapes.pins"] .cdx-message--error' ) ).toContainText( 'not JSON' );
	await expect( page.locator( '[id="ts-p-shapes.pins"]' ) ).toHaveValue( '{' );
	await expect( page.locator( '#statements .cdx-message--success' ) ).toHaveCount( 0 );
	// Saved: the section says so, and the row offers Reset.
	await page.goto( '/wiki/Special:Preferences' );
	await page.locator( '[id="ts-p-rc.limit"]' ).fill( '20' );
	await page.locator( '#history' ).getByRole( 'button', { name: 'Save', exact: true } ).click();
	await expect( page.locator( '#history .cdx-message--success' ) )
		.toContainText( 'Your preferences have been saved.' );
	await expect( page.locator( '[id="ts-p-rc.limit"]' ) ).toHaveValue( '20' );
	const reset = page.locator( '[id="pref-rc.limit"] .ts-setting__reset' );
	await expect( reset ).toBeVisible();
	// Reset: the value returns to the default, and the row no longer offers Reset.
	await reset.click();
	await expect( page.locator( '#history .cdx-message--success' ) ).toBeVisible();
	await expect( page.locator( '[id="ts-p-rc.limit"]' ) ).toHaveValue( '50' );
	await expect( page.locator( '[id="pref-rc.limit"] .ts-setting__reset' ) ).toHaveCount( 0 );
} );
