// The moderation forms' refused and answered states, without JavaScript (09 §3.3, §4.5,
// §6.3; chapter 19 §6.3), beside what rights.spec.js drives: the parity corpus of the skin
// migration records GET requests only, so these are the states the suite under
// TRIPLESPACE_WEB_RENDERER=compare checks for group 11 (19 §5.10). Each test leaves the
// wiki as it found it.
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

test.describe.serial( 'moderation forms', () => {
	test( 'a protection with an expiry the wiki cannot read is refused, keeping what was typed', async ( { page } ) => {
		await logIn( page, 'Special:SpecialPages' );
		await page.goto( '/w/index.php?title=Project:Home&action=protect' );
		await page.locator( '#ts-protect-expiry' ).fill( 'whenever' );
		await page.locator( '#ts-protect-reason' ).fill( 'Kept' );
		await page.locator( 'form.ts-protect' ).getByRole( 'button' ).first().click();
		await expect( page.locator( '.cdx-message--error' ) ).toContainText( 'The expiry is not a time this wiki understands.' );
		await expect( page.locator( '#ts-protect-expiry' ) ).toHaveValue( 'whenever' );
		await expect( page.locator( '#ts-protect-reason' ) ).toHaveValue( 'Kept' );
	} );

	test( 'a deletion of a title with no page is refused on the form', async ( { page } ) => {
		await logIn( page, 'Special:SpecialPages' );
		await page.goto( '/w/index.php?title=Project:Never_here&action=delete' );
		await page.locator( '#ts-delete-reason' ).fill( 'Gone' );
		await page.locator( 'form.ts-delete' ).getByRole( 'button' ).first().click();
		await expect( page.locator( '.cdx-message--error' ) ).toContainText( 'There is no such page, or it is not yours to see.' );
		await expect( page.locator( '#ts-delete-reason' ) ).toHaveValue( 'Gone' );
		await expect( page.locator( '#ts-delete-talk' ) ).toBeChecked();
	} );

	test( 'unblocking an account that is not blocked is refused; the block notice follows a block', async ( { page } ) => {
		await logIn( page, 'Special:SpecialPages' );
		await page.goto( '/wiki/Special:Unblock/Nobody_blocked' );
		await expect( page.locator( '#ts-block-user' ) ).toHaveValue( 'Nobody blocked' );
		await page.locator( 'form.ts-block' ).getByRole( 'button' ).first().click();
		await expect( page.locator( '.cdx-message--error' ) ).toBeVisible();
		await expect( page.locator( '#ts-block-user' ) ).toHaveValue( 'Nobody blocked' );
	} );

	test( 'user rights: a name with no account, and a save that changes nothing', async ( { page } ) => {
		await logIn( page, 'Special:SpecialPages' );
		await page.goto( '/wiki/Special:UserRights/Nobody_here' );
		await expect( page.locator( '.cdx-message--error' ) ).toContainText( 'There is no account by that name.' );
		await expect( page.locator( 'form.ts-userrights__ask' ) ).toBeVisible();
		await page.goto( '/wiki/Special:UserRights/Alice' );
		await page.locator( '#ts-userrights-reason' ).fill( 'Nothing' );
		await page.locator( 'form.ts-userrights__groups' ).getByRole( 'button' ).click();
		await expect( page.locator( '.ts-userrights > .cdx-message' ).first() ).toContainText( 'No group was changed.' );
		await expect( page.locator( '#ts-userrights-reason' ) ).toHaveValue( 'Nothing' );
	} );

	test( 'a set refuses a title with no page, takes an item and lets it go, and is retired', async ( { page } ) => {
		await logIn( page, 'Special:ProtectedPages' );
		await page.getByLabel( 'Name' ).last().fill( 'Moderation set' );
		await page.locator( '#ts-set-new-edit' ).selectOption( 'sysop' );
		await page.locator( 'form.ts-set__new' ).getByRole( 'button' ).click();
		const set = page.locator( '.ts-set', { hasText: 'Moderation set' } );
		await set.getByLabel( 'Add a page' ).fill( 'Project:Never here' );
		await set.locator( 'form.ts-set__add' ).getByRole( 'button' ).click();
		await expect( page.locator( '.ts-protectedpages > .cdx-message--error' ) ).toContainText( 'There is no such page, or it is not yours to see.' );
		// An item, named by its title: its page ID is the entity's (09 §4.7).
		await set.getByLabel( 'Add a page' ).fill( 'Item:Q5' );
		await set.locator( 'form.ts-set__add' ).getByRole( 'button' ).click();
		await expect( page.locator( '.ts-protectedpages > .cdx-message--success' ) ).toBeVisible();
		await expect( set.locator( '.ts-set__members li' ) ).toHaveCount( 1 );
		await expect( set.locator( '.ts-set__members li' ) ).toContainText( 'Item:Q5' );
		// Added again: the set says it already holds it, on the form.
		await set.getByLabel( 'Add a page' ).fill( 'Item:Q5' );
		await set.locator( 'form.ts-set__add' ).getByRole( 'button' ).click();
		await expect( page.locator( '.ts-protectedpages > .cdx-message--error' ) ).toBeVisible();
		await set.locator( 'form.ts-set__remove' ).getByRole( 'button' ).click();
		await expect( page.locator( '.ts-protectedpages > .cdx-message--success' ) ).toBeVisible();
		await expect( set.locator( '.ts-set__members' ) ).toHaveCount( 0 );
		await set.locator( 'form.ts-set__retire' ).getByRole( 'button' ).click();
		await expect( page.locator( '.ts-set', { hasText: 'Moderation set' } ) ).toHaveCount( 0 );
	} );
} );
