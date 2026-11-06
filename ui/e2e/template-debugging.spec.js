// Template debugging (ADR 0042 §4; 0034 §9): `uselang=qqx` shows interface message keys,
// and `tstemplates=1` marks which template drew each block of a page.
import { test, expect } from '@playwright/test';

const PAGE = 'Project:Template_debugging';

test( 'uselang=qqx shows the interface messages\' keys', async ( { page } ) => {
	await page.goto( '/wiki/Item:Q6?uselang=qqx' );
	await expect( page.getByRole( 'link', { name: '(ts-tab-history)' } ) ).toBeVisible();
	await expect( page.getByRole( 'link', { name: '(ts-nav-recentchanges)' } ) ).toBeVisible();
	// The content is the content: the item's label is not a message.
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toContainText( 'Six & more' );
	// Without it, the words.
	await page.goto( '/wiki/Item:Q6' );
	await expect( page.getByRole( 'link', { name: 'History' } ) ).toBeVisible();
} );

test.describe.serial( 'a page that transcludes templates', () => {
	test( 'is written, with its templates', async ( { page } ) => {
		await page.goto( '/wiki/Special:UserLogin?returnto=Project:Home' );
		await page.getByLabel( 'Username' ).fill( 'Alice' );
		await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
		await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
		for ( const [ title, text ] of [
			[ 'Template:Debug_box', '<div class="debug-box">Boxed by a template.</div>' ],
			[ 'Template:Debug_word', 'templated' ],
			[ PAGE, 'A sentence with a {{debug word}} word.\n\n{{Debug box}}' ]
		] ) {
			await page.goto( `/w/index.php?title=${ title }&action=edit` );
			await page.locator( '#wpTextbox1' ).fill( text );
			await page.locator( 'button[name="wpSave"]' ).click();
			await expect( page.locator( '.mw-parser-output' ) ).toBeVisible();
		}
	} );

	test( 'marks a template\'s blocks only when asked', async ( { page } ) => {
		await page.goto( `/wiki/${ PAGE }` );
		await expect( page.locator( '.debug-box' ) ).toBeVisible();
		await expect( page.locator( '[data-ts-template]' ) ).toHaveCount( 0 );

		await page.goto( `/wiki/${ PAGE }?tstemplates=1` );
		const box = page.locator( '.debug-box' );
		await expect( box ).toHaveAttribute( 'data-ts-template', 'Template:Debug box' );
		// Where the call starts in the page's text, in bytes.
		const at = 'A sentence with a {{debug word}} word.\n\n'.length;
		await expect( box ).toHaveAttribute( 'data-ts-template-at', String( at ) );
		// A template's words inside a paragraph mark nothing.
		await expect( page.locator( '[data-ts-template]' ) ).toHaveCount( 1 );
	} );
} );
