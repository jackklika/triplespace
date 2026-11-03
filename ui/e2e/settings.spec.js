// The settings pages' POSTed states, without JavaScript (0085 §4; 09 §4.7; chapter 19
// §2.8): what a refused save, a save, a Reset, a refused Visibility choice and the
// instance's Defaults for wikis with a lock answer. The skin migration's parity corpus
// records GET requests only, so these are the states that the suite under
// TRIPLESPACE_WEB_RENDERER=compare checks for group 10 (19 §5.10). Each test leaves the
// settings as it found them.
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

/**
 * The element with this ID, which has dots in it.
 *
 * @param {Object} page A Playwright page
 * @param {string} id The ID
 * @return {Object} A locator
 */
function byId( page, id ) {
	return page.locator( `[id="${ id }"]` );
}

test.describe.serial( 'settings', () => {
	test( 'a refused setting comes back with the reader\'s text and why; a good one saves and resets', async ( { page } ) => {
		await logIn( page, 'Special:WikiSettings' );
		// Refused by the API: the reader's text back in the field, the reason on its row.
		const expiry = byId( page, 'ts-s-account.request_expiry' );
		await expiry.fill( 'soon' );
		await page.locator( '#accounts' ).getByRole( 'button', { name: 'Save', exact: true } ).click();
		await expect( byId( page, 'setting-account.request_expiry' ).locator( '.cdx-message--error' ) ).toBeVisible();
		await expect( expiry ).toHaveValue( 'soon' );
		await expect( page.locator( '#accounts .cdx-message--success' ) ).toHaveCount( 0 );
		const keep = byId( page, 'ts-s-dumps.keep' );
		const row = byId( page, 'setting-dumps.keep' );
		// Saved: the section says so, the chip says where the value comes from, and the
		// row offers Reset.
		await page.goto( '/wiki/Special:WikiSettings' );
		await keep.fill( '9' );
		await page.locator( '#exchange' ).getByRole( 'button', { name: 'Save', exact: true } ).click();
		await expect( page.locator( '#exchange .cdx-message--success' ) ).toContainText( 'Saved.' );
		await expect( keep ).toHaveValue( '9' );
		await expect( row.locator( '.ts-setting__origin' ) ).toHaveText( 'Set on this wiki' );
		// Reset: the registry's default again, and no Reset.
		await row.locator( '.ts-setting__reset' ).click();
		await expect( page.locator( '#exchange .cdx-message--success' ) ).toBeVisible();
		await expect( keep ).toHaveValue( '7' );
		await expect( row.locator( '.ts-setting__origin' ) ).toHaveText( 'Default' );
		await expect( row.locator( '.ts-setting__reset' ) ).toHaveCount( 0 );
	} );

	test( 'a group without a group is refused, keeping the reader\'s choice', async ( { page } ) => {
		await logIn( page, 'Special:WikiSettings' );
		const section = page.locator( '#visibility' );
		await section.getByLabel( 'A group', { exact: true } ).check();
		await section.getByRole( 'button', { name: 'Save', exact: true } ).click();
		await expect( section.locator( '.cdx-message--error' ) ).toContainText( 'Choose a group.' );
		await expect( page.locator( '#ts-vis-group' ) ).toBeChecked();
		await expect( section ).toContainText( 'Who may read this wiki now: Anyone' );
	} );

	test( 'the instance locks a default for wikis, and a refused default says why', async ( { page } ) => {
		await logIn( page, 'Special:InstanceSettings' );
		const template = byId( page, 'ts-t-config.template' );
		const was = await template.inputValue();
		const saveIn = ( id ) => page.locator( `#${ id }` ).getByRole( 'button', { name: 'Save', exact: true } ).click();
		await template.selectOption( 'locks' );
		await saveIn( 'tenancy' );
		await expect( page.locator( '#tenancy .cdx-message--success' ) ).toBeVisible();
		const keep = byId( page, 'ts-d-dumps.keep' );
		const lock = byId( page, 'ts-l-dumps.keep' );
		const row = byId( page, 'default-dumps.keep' );
		// Refused: the reader's text, and the reason after the row's help.
		const expiry = byId( page, 'ts-d-account.request_expiry' );
		await expiry.fill( 'soon' );
		await saveIn( 'defaults' );
		await expect( byId( page, 'default-account.request_expiry' ).locator( '.ts-setting__help + .cdx-message--error' ) ).toBeVisible();
		await expect( expiry ).toHaveValue( 'soon' );
		// A default, locked: the wiki shows it as the instance's, locked.
		await page.goto( '/wiki/Special:InstanceSettings' );
		await keep.fill( '14' );
		await lock.check();
		await saveIn( 'defaults' );
		await expect( page.locator( '#defaults .cdx-message--success' ) ).toBeVisible();
		await expect( keep ).toHaveValue( '14' );
		await expect( lock ).toBeChecked();
		await page.goto( '/wiki/Special:WikiSettings' );
		await expect( byId( page, 'setting-dumps.keep' ) ).toContainText( 'Locked by the instance' );
		await expect( byId( page, 'setting-dumps.keep' ) ).toContainText( '14' );
		await expect( byId( page, 'ts-s-dumps.keep' ) ).toHaveCount( 0 );
		// As it was: no default, no lock, the template as found.
		await page.goto( '/wiki/Special:InstanceSettings' );
		await row.locator( '.ts-setting__reset' ).click();
		await expect( keep ).toHaveValue( '' );
		await lock.uncheck();
		await saveIn( 'defaults' );
		await expect( lock ).not.toBeChecked();
		await template.selectOption( was );
		await saveIn( 'tenancy' );
		await expect( template ).toHaveValue( was );
	} );
} );
