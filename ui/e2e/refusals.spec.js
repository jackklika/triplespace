// The refused states of group 12's forms, without JavaScript (19 §5.10): the parity corpus
// records GET requests only, so these are the POST answers the suite under
// TRIPLESPACE_WEB_RENDERER=compare checks: Special:NewItem and Special:SetLabel with a
// token the session never issued, and Special:Account refusing to remove a sign-in method.
// A form is sent as the browser would send it, with its own fields and one changed, and
// the answer is read as the page the browser would show. Nothing here changes the wiki.
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
 * Sends the page's form with some fields changed, and shows the answer in the page.
 *
 * @param {Object} page A Playwright page showing the form
 * @param {string} selector The form
 * @param {Object} changed The fields to change
 * @return {Promise<number>} The answer's status
 */
async function sendChanged( page, selector, changed ) {
	const form = page.locator( selector ).first();
	const action = await form.getAttribute( 'action' );
	const fields = {};
	for ( const input of await form.locator( 'input[name], textarea[name], select[name]' ).all() ) {
		const type = await input.getAttribute( 'type' );
		if ( ( type === 'checkbox' || type === 'radio' ) && !( await input.isChecked() ) ) {
			continue;
		}
		fields[ await input.getAttribute( 'name' ) ] = await input.inputValue();
	}
	const r = await page.request.post( action, { form: { ...fields, ...changed }, maxRedirects: 0 } );
	await page.setContent( await r.text() );
	return r.status();
}

test.describe.serial( 'refused forms', () => {
	test( 'Special:NewItem with an expired token keeps what was typed', async ( { page } ) => {
		await logIn( page, 'Special:NewItem' );
		await page.getByLabel( 'Label', { exact: true } ).fill( 'Never created' );
		await sendChanged( page, 'form.ts-form', { label: 'Never created', token: 'expired+\\' } );
		await expect( page.locator( '.cdx-message--warning' ) ).toContainText( 'Your session expired before the form was sent.' );
		await expect( page.getByLabel( 'Label', { exact: true } ) ).toHaveValue( 'Never created' );
		await expect( page.locator( 'input[name="token"]' ) ).not.toHaveValue( 'expired+\\' );
	} );

	test( 'Special:SetLabel with an expired token says so, on the form', async ( { page } ) => {
		await logIn( page, 'Special:SetLabel/Q6/en' );
		await sendChanged( page, 'form.ts-form', { label: 'Never set', token: 'expired+\\' } );
		await expect( page.locator( '.cdx-message--error' ) ).toContainText( 'Your session ended before the form was sent.' );
		await expect( page.locator( 'form.ts-form' ) ).toBeVisible();
	} );

	test( 'Special:Account refuses to remove a way to log in it was not asked for properly', async ( { page } ) => {
		await logIn( page, 'Special:Account' );
		await page.goto( '/wiki/Special:Account' );
		const token = await page.locator( 'section.ts-signin input[name="token"]' ).first().inputValue();
		const r = await page.request.post( '/w/index.php?title=Special:Account', {
			form: { issuer: 'wikimedia-central', token: 'expired+\\' },
			maxRedirects: 0
		} );
		await page.setContent( await r.text() );
		await expect( page.locator( '.ts-account > .cdx-message--error' ) ).toContainText( 'Your session expired before the form was sent.' );
		await expect( page.getByText( 'Password on this wiki' ) ).toBeVisible();
		// With the session's own token: Alice holds no such binding, so the API refuses.
		const s = await page.request.post( '/w/index.php?title=Special:Account', {
			form: { issuer: 'wikimedia-central', token },
			maxRedirects: 0
		} );
		await page.setContent( await s.text() );
		await expect( page.locator( '.ts-account > .cdx-message--error' ) ).toBeVisible();
	} );
} );
