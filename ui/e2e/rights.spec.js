// The Protect form, Special:UserRights, Special:ProtectedPages, Special:Block,
// Special:MovePage and the Visibility section (chapter 19 §6.3), with JavaScript off, as
// the owner and as a reader, through the edge; axe on each form.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

/**
 * Logs the owner in and lands on a page.
 *
 * @param {Object} page A Playwright page
 * @param {string} back The title to return to
 */
async function logIn( page, back ) {
	await page.goto( `/wiki/Special:UserLogin?returnto=${ back }` );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
}

/**
 * The axe violations on the current page.
 *
 * @param {Object} page A Playwright page
 * @return {Promise<string[]>} The violations, as `id: targets`
 */
async function violations( page ) {
	const results = await new AxeBuilder( { page } )
		.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
		.analyze();
	return results.violations.map(
		( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
	);
}

test( 'a reader is asked to log in, and sees no Protect tab', async ( { page } ) => {
	await page.goto( '/wiki/Item:Q6' );
	await expect( page.locator( '.ts-tabs__more' ) ).toHaveCount( 0 );
	await page.goto( '/w/index.php?title=Project:Home&action=protect' );
	await expect( page.getByText( 'Log in to protect pages.' ) ).toBeVisible();
	await page.goto( '/wiki/Special:UserRights' );
	await expect( page.getByText( 'Log in to change user rights.' ) ).toBeVisible();
	await page.goto( '/wiki/Special:ProtectedPages' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Protected pages' );
	await expect( page.locator( 'input[name="do"]' ) ).toHaveCount( 0 );
} );

test.describe( 'the owner', () => {
	test( 'protects a page to a group and sees it listed', async ( { page } ) => {
		await logIn( page, 'Project:Home' );
		// A group to restrict to, which the owner joins through Special:UserRights.
		await page.goto( '/wiki/Special:UserRights/Alice' );
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'User rights of Alice' );
		await expect( page.getByLabel( 'New memberships expire' ) ).toHaveValue( '1 day' );
		await page.getByLabel( 'sysop', { exact: true } ).check();
		await page.getByLabel( 'New memberships expire' ).fill( 'infinite' );
		await page.getByLabel( 'Reason' ).fill( 'e2e' );
		await page.getByRole( 'button', { name: 'Save user groups' } ).click();
		await expect( page.getByText( 'The groups were changed.' ) ).toBeVisible();
		await expect( page.getByLabel( 'sysop', { exact: true } ) ).toBeChecked();

		// An entity page carries the tab; the main page is the home page, with none.
		await page.goto( '/wiki/Item:Q6' );
		// The less common actions sit in the overflow menu beside the tabs (19 §1.3).
		await page.locator( '.ts-tabs__more > summary' ).click();
		await page.getByRole( 'link', { name: 'Protect…' } ).click();
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Protect Item:Q6' );
		await expect( page.getByLabel( 'Also restrict the talk page' ) ).toBeChecked();
		await page.getByLabel( 'Edit', { exact: true } ).selectOption( 'sysop' );
		await page.getByLabel( 'Read', { exact: true } ).selectOption( 'sysop' );
		await page.getByLabel( 'Reason' ).fill( 'e2e' );
		await page.getByRole( 'button', { name: 'Save' } ).click();
		await expect( page.getByText( 'The protection was saved.' ) ).toBeVisible();
		await expect( page.getByLabel( 'Read', { exact: true } ) ).toHaveValue( 'sysop' );
		// The item now wears the Restricted pill, naming the group.
		await page.goto( '/wiki/Item:Q6' );
		const pill = page.locator( '.ts-chip--restricted' );
		await expect( pill ).toBeVisible();
		await expect( pill ).toHaveAttribute( 'title', /sysop/ );

		await page.goto( '/wiki/Special:ProtectedPages' );
		const row = page.locator( '.ts-protectedpages__table tr', { hasText: 'Read' } );
		await expect( row ).toContainText( 'sysop' );

		// Lifted again, so the other suites find the item.
		await page.goto( '/w/index.php?title=Item:Q6&action=protect' );
		await page.getByLabel( 'Edit', { exact: true } ).selectOption( '' );
		await page.getByLabel( 'Read', { exact: true } ).selectOption( '' );
		await page.getByRole( 'button', { name: 'Save' } ).click();
		await expect( page.getByText( 'The protection was saved.' ) ).toBeVisible();
	} );

	test( 'is locked out of a group it is not in', async ( { page } ) => {
		await logIn( page, 'Project:Home' );
		await page.goto( '/w/index.php?title=Item:Q6&action=protect' );
		await page.getByLabel( 'Read', { exact: true } ).selectOption( 'bureaucrat' );
		await page.getByRole( 'button', { name: 'Save' } ).click();
		await expect( page.getByText( 'You must be a member of the group you restrict reading to' ) ).toBeVisible();
		await expect( page.getByLabel( 'Read', { exact: true } ) ).toHaveValue( 'bureaucrat' );
	} );

	test( 'the block form offers hiding only with the right, and refuses a temporary hide', async ( { page } ) => {
		await logIn( page, 'Special:Block' );
		await page.goto( '/wiki/Special:Block' );
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Block an account' );
		await expect( page.getByLabel( 'Hide the username' ) ).toBeVisible();
		await page.getByLabel( 'Account', { exact: true } ).fill( 'Alice' );
		await page.getByRole( 'button', { name: 'Block' } ).click();
		await expect( page.getByText( 'You cannot block yourself.' ) ).toBeVisible();
	} );

	test( 'deletes a page, sees its deletion, and undeletes it', async ( { page } ) => {
		await logIn( page, 'Project:Home' );
		// A page of our own, through the edit form.
		await page.goto( '/w/index.php?title=Project:Doomed&action=edit' );
		await page.locator( '#wpTextbox1' ).fill( 'Soon gone.' );
		await page.locator( 'button[name="wpSave"]' ).click();
		// The project namespace is shown under the wiki's own name.
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'librarybase:Doomed' );
		await page.locator( '.ts-tabs__more > summary' ).click();
		await page.getByRole( 'link', { name: 'Delete…' } ).click();
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Delete librarybase:Doomed' );
		await page.getByLabel( 'Reason' ).fill( 'e2e' );
		await page.getByRole( 'button', { name: 'Delete', exact: true } ).click();
		// The deleted page: the banner, the content for the group, the way back.
		await expect( page.getByText( 'This page was deleted by Alice' ) ).toBeVisible();
		await expect( page.getByText( 'Soon gone.' ) ).toBeVisible();
		await page.goto( '/wiki/Special:Log/delete' );
		await expect( page.locator( '.ts-change--log' ).first() ).toContainText( 'Doomed' );
		// A deleted page's edits leave recent changes, for its deleter too (ADR 0020 A19).
		await page.goto( '/wiki/Special:RecentChanges' );
		await expect( page.locator( '.ts-change--edit', { hasText: 'Doomed' } ) ).toHaveCount( 0 );
		await page.goto( '/wiki/Special:Undelete' );
		await page.getByRole( 'link', { name: 'Project:Doomed' } ).click();
		await page.getByRole( 'button', { name: 'Undelete' } ).click();
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'librarybase:Doomed' );
		await expect( page.getByText( 'Soon gone.' ) ).toBeVisible();
		await expect( page.getByText( 'This page was deleted' ) ).toHaveCount( 0 );
	} );

	test( 'moves a page, leaving a redirect', async ( { page } ) => {
		await logIn( page, 'Project:Home' );
		await page.goto( '/w/index.php?title=Project:Movable&action=edit' );
		await page.locator( '#wpTextbox1' ).fill( 'On the move.' );
		await page.locator( 'button[name="wpSave"]' ).click();
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'librarybase:Movable' );
		await page.locator( '.ts-tabs__more > summary' ).click();
		await page.getByRole( 'link', { name: 'Move…' } ).click();
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Move librarybase:Movable' );
		// The owner may suppress the redirect; the box is on by default.
		await expect( page.getByLabel( 'Leave a redirect behind' ) ).toBeChecked();
		await page.getByLabel( 'New title' ).fill( 'Project:Moved' );
		await page.getByLabel( 'Reason' ).fill( 'e2e' );
		await page.getByRole( 'button', { name: 'Move page' } ).click();
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'librarybase:Moved' );
		await expect( page.getByText( 'On the move.' ) ).toBeVisible();
		await page.goto( '/w/index.php?title=Project:Movable&action=edit' );
		await expect( page.locator( '#wpTextbox1' ) ).toHaveValue( '#REDIRECT [[librarybase:Moved]]' );
		await page.goto( '/wiki/Special:Log/move' );
		await expect( page.locator( '.ts-change' ).first() ).toContainText( 'Project:Movable → Project:Moved' );
		// A title that is taken is refused over the form.
		await page.goto( '/w/index.php?title=Project:Taken&action=edit' );
		await page.locator( '#wpTextbox1' ).fill( 'Taken.' );
		await page.locator( 'button[name="wpSave"]' ).click();
		await page.goto( '/wiki/Special:MovePage/Project:Moved' );
		await page.getByLabel( 'New title' ).fill( 'Project:Taken' );
		await page.getByRole( 'button', { name: 'Move page' } ).click();
		await expect( page.getByText( 'A page with that title already exists.' ) ).toBeVisible();
	} );

	test( 'a set is created, renamed and retired on Special:ProtectedPages', async ( { page } ) => {
		await logIn( page, 'Special:ProtectedPages' );
		await page.goto( '/wiki/Special:ProtectedPages' );
		await page.getByLabel( 'Name' ).last().fill( 'E2E set' );
		await page.locator( '#ts-set-new-edit' ).selectOption( 'sysop' );
		await page.getByRole( 'button', { name: 'Create set' } ).click();
		await expect( page.getByText( 'The set was created.' ) ).toBeVisible();
		const set = page.locator( '.ts-set', { hasText: 'E2E set' } );
		await set.getByLabel( 'Name' ).fill( 'E2E set renamed' );
		await set.getByRole( 'button', { name: 'Save set' } ).click();
		await expect( page.getByText( 'The set was changed.' ) ).toBeVisible();
		const renamed = page.locator( '.ts-set', { hasText: 'E2E set renamed' } );
		await expect( renamed ).toBeVisible();
		await renamed.getByRole( 'button', { name: 'Retire this set' } ).click();
		await expect( page.getByText( 'The set was retired.' ) ).toBeVisible();
	} );

	test( 'the block list shows a block and offers to lift it', async ( { page } ) => {
		await logIn( page, 'Special:BlockList' );
		await page.goto( '/wiki/Special:BlockList' );
		await expect( page.getByText( 'No account is blocked.' ) ).toBeVisible();
	} );

	// axe runs as a script in the page, so these contexts allow scripts; the site's own
	// script only marks the page as having them.
	for ( const path of [
		'/w/index.php?title=Item:Q6&action=protect',
		'/wiki/Special:UserRights/Alice',
		'/wiki/Special:ProtectedPages',
		'/wiki/Special:Block',
		'/wiki/Special:BlockList',
		'/wiki/Special:Log',
		'/wiki/Special:Undelete',
		'/w/index.php?title=Item:Q6&action=delete',
		'/wiki/Special:MovePage/Project:Home'
	] ) {
		test( `${ path } passes axe`, async ( { browser } ) => {
			const context = await browser.newContext( { javaScriptEnabled: true } );
			const page = await context.newPage();
			await logIn( page, 'Project:Home' );
			await page.goto( path );
			await expect( page.locator( 'form.ts-protect, form.ts-userrights__groups, .ts-protectedpages, form.ts-block, .ts-changes, form.ts-delete, form.ts-move' ).first() ).toBeVisible();
			expect( await violations( page ) ).toEqual( [] );
			await context.close();
		} );
	}

	test( 'the Visibility section reports the wiki as public', async ( { page } ) => {
		await logIn( page, 'Special:WikiSettings' );
		await page.goto( '/wiki/Special:WikiSettings' );
		await expect( page.getByText( 'Who may read this wiki now: Anyone' ) ).toBeVisible();
		await expect( page.getByLabel( 'Accounts on this wiki' ) ).toBeVisible();
	} );
} );
