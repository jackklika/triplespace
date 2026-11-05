// Links in rendered content (ADR 0086 §2, A12): a red link meets 4.5:1 on the page in
// light and dark mode, and a link inside running text is underlined, so it is told
// apart from the text around it without colour (WCAG 1.4.1).
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

const TITLE = 'Project:Content_links';
const TEXT = [
	'A sentence with [[No such page at all]], a red link, and [[Item:Q6|a blue link]] inside it.',
	'',
	'* A list item that links to [[Item:Q6|an item]] mid-sentence.'
].join( '\n' );

test.describe.serial( 'links in a page', () => {
	test( 'are written through the edit form', async ( { page } ) => {
		await page.goto( '/wiki/Special:UserLogin?returnto=Project:Home' );
		await page.getByLabel( 'Username' ).fill( 'Alice' );
		await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
		await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
		await page.goto( `/w/index.php?title=${ TITLE }&action=edit` );
		await page.locator( '#wpTextbox1' ).fill( TEXT );
		await page.locator( 'button[name="wpSave"]' ).click();
		await expect( page.locator( '.mw-parser-output a.new' ) ).toHaveCount( 1 );
	} );

	for ( const colorScheme of [ 'light', 'dark' ] ) {
		// axe runs as a script, so this context allows scripts.
		test( `pass axe in ${ colorScheme } mode, red link and links in text included`, async ( { browser } ) => {
			const context = await browser.newContext( { javaScriptEnabled: true, colorScheme } );
			const page = await context.newPage();
			await page.goto( `/wiki/${ TITLE }` );
			// The red link is Codex's red-link colour, which the palette's sheet now
			// defines, not a stylesheet's fallback.
			const red = page.locator( '.mw-parser-output a.new' );
			const token = await page.evaluate( () => getComputedStyle( document.documentElement ).getPropertyValue( '--color-destructive' ).trim() );
			expect( token ).not.toBe( '' );
			await expect( red ).toHaveCSS( 'text-decoration-line', 'underline' );
			await expect( page.locator( '.mw-parser-output p a:not(.new)' ) ).toHaveCSS( 'text-decoration-line', 'underline' );
			const results = await new AxeBuilder( { page } )
				.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
				.analyze();
			const found = results.violations.map(
				( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
			);
			expect( found ).toEqual( [] );
			await context.close();
		} );
	}

	test( 'leave links outside content as they were', async ( { page } ) => {
		await page.goto( `/wiki/${ TITLE }` );
		await expect( page.getByRole( 'link', { name: 'Recent changes' } ) ).toHaveCSS( 'text-decoration-line', 'none' );
	} );
} );
