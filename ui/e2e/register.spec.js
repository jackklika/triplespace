// Registration (ADR 0087 §5): without JavaScript the form says what the wiki asks of the
// browser and why, and a submission is refused with that explanation; with it, the
// browser solves the challenge in a Web Worker while the person types, a submission made
// before then is held until the solution comes, and the new account is logged in. The
// form passes axe.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test( 'without JavaScript, registration explains what it asks for', async ( { page } ) => {
	await page.goto( '/wiki/Special:CreateAccount' );
	await expect( page.getByRole( 'heading', { name: 'Create account' } ) ).toBeVisible();
	// Playwright's text search passes over <noscript>, so the block is found by its markup.
	const explained = page.locator( 'noscript .cdx-message__content' );
	await expect( explained ).toBeVisible();
	expect( await explained.innerText() ).toContain( 'Turn on JavaScript for this site' );
	// The honeypot is out of sight and out of the accessibility tree.
	await expect( page.locator( '#wpWebsite' ) ).not.toBeInViewport();
	await page.getByLabel( 'Username' ).fill( 'Nojs Reader' );
	await page.getByLabel( 'Password', { exact: true } ).fill( 'nojs-password-1' );
	await page.getByLabel( 'Confirm password' ).fill( 'nojs-password-1' );
	await page.getByRole( 'button', { name: 'Create your account' } ).click();
	await expect( page.locator( '.cdx-message--error' ) ).toContainText( 'Turn on JavaScript for this site' );
	await expect( page.getByLabel( 'Username' ) ).toHaveValue( 'Nojs Reader' );
} );

test( 'the form carries what the challenge script reads', async ( { page } ) => {
	// The contract with ui/src/challenge/form.js, whichever renderer drew the form (19 §5.10).
	await page.goto( '/wiki/Special:CreateAccount' );
	const form = page.locator( 'form[data-ts-challenge]' );
	await expect( form ).toHaveCount( 1 );
	const bits = await form.getAttribute( 'data-ts-challenge' );
	expect( Number( bits ) ).toBeGreaterThan( 0 );
	const id = form.locator( 'input[type="hidden"][name="tschallengeid"]' );
	await expect( id ).toHaveAttribute( 'data-ts-challenge-bits', bits );
	expect( await id.inputValue() ).not.toBe( '' );
	await expect( form.locator( 'input[type="hidden"][name="tschallengesolution"]' ) ).toHaveValue( '' );
	const status = form.locator( '[data-ts-challenge-status]' );
	await expect( status ).toHaveAttribute( 'role', 'status' );
	await expect( status ).toHaveAttribute( 'aria-live', 'polite' );
	for ( const attribute of [ 'data-working', 'data-waiting', 'data-done', 'data-failed' ] ) {
		expect( await status.getAttribute( attribute ) ).toBeTruthy();
	}
} );

test.describe( 'with JavaScript', () => {
	test.use( { javaScriptEnabled: true } );

	test( 'the browser solves the challenge and the new account is logged in', async ( { page } ) => {
		await page.goto( '/wiki/Special:CreateAccount?returnto=Special:SpecialPages' );
		const status = page.locator( '[data-ts-challenge-status]' );
		await page.getByLabel( 'Username' ).fill( 'Worker Reader' );
		await page.getByLabel( 'Password', { exact: true } ).fill( 'worker-password-1' );
		await page.getByLabel( 'Confirm password' ).fill( 'worker-password-1' );
		// Submitted at once: held until the solution comes, then sent.
		await page.getByRole( 'button', { name: 'Create your account' } ).click();
		await page.waitForURL( '**/wiki/Special:SpecialPages', { timeout: 60000 } );
		await expect( page.locator( '.ts-header__end' ) ).toContainText( 'Worker Reader' );
		expect( await status.count() ).toBe( 0 );
	} );

	test( 'the form passes axe while the browser works', async ( { page } ) => {
		await page.goto( '/wiki/Special:CreateAccount' );
		await expect( page.locator( '[data-ts-challenge-status]' ) ).not.toBeEmpty();
		const results = await new AxeBuilder( { page } )
			.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
			.analyze();
		const found = results.violations.map(
			( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
		);
		expect( found ).toEqual( [] );
	} );
} );
