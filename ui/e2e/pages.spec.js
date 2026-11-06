// Every page kind of Phase 1 with JavaScript off (ADR 0034 §1.1), checked by axe (§10),
// through an edge that alternates two web replicas, so a page that needs one replica's
// memory fails here.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

/**
 * How far the page scrolls sideways: 0 unless something overflows.
 *
 * @param {Object} page A Playwright page
 * @return {Promise<number>} The overflow in pixels
 */
function overflow( page ) {
	return page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
}

const pages = [
	{ path: '/wiki/Project:Home', heading: 'librarybase' },
	{ path: '/wiki/Special:SpecialPages', heading: 'Special pages' },
	{ path: '/wiki/Special:UserLogin', heading: 'Log in' },
	{ path: '/wiki/Project:About', heading: 'Project:About', status: 404 },
	{ path: '/wiki/Project:About?action=edit', heading: 'Project:About' },
	{ path: '/wiki/Project:About?action=history', heading: 'Project:About', status: 404 },
	{ path: '/w/index.php?action=edit', heading: 'New page' },
	{ path: '/wiki/Item:Q6', heading: 'Six & more' },
	{ path: '/wiki/Item:Q6?tab=identifiers', heading: 'Six & more' },
	{ path: '/wiki/Item:Q6?tab=sitelinks', heading: 'Six & more' },
	{ path: '/wiki/Item:Q6?tab=labels', heading: 'Six & more' },
	{ path: '/wiki/Item:Q6?uselang=ar', heading: 'ستة' },
	{ path: '/wiki/Property:P3', heading: 'population' },
	{ path: '/wiki/Domain:wikipedia.org', heading: 'Wikipedia' },
	{ path: '/wiki/Item:Q404', heading: 'Item:Q404', status: 404 },
	{ path: '/w/index.php?title=Special:Search&search=six&fulltext=1', heading: 'Search' },
	{ path: '/wiki/Special:Search', heading: 'Search' },
	{ path: '/wiki/Special:NewItem', heading: 'Create a new item' },
	{ path: '/wiki/Special:Version', heading: 'Version' },
	{ path: '/wiki/Special:Version/Credits', heading: 'Credits' },
	{ path: '/wiki/Special:Version/License', heading: 'GNU General Public License' },
	{ path: '/wiki/Special:RecentChanges', heading: 'Recent changes' },
	{ path: '/w/index.php?title=Special:RecentChanges&filtered=1&kind=edit&kind=sync&days=90', heading: 'Recent changes' },
	{ path: '/wiki/Special:Contributions', heading: 'User contributions' },
	{ path: '/wiki/Special:Contributions/Alice', heading: 'Contributions of Alice' },
	{ path: '/wiki/Item:Q6?action=history', heading: 'Six & more' },
	{ path: '/wiki/Domain:wikipedia.org?action=history', heading: 'Wikipedia' },
	{ path: '/wiki/Special:WikiSettings', heading: 'Wiki settings' },
	{ path: '/wiki/Special:UserRights', heading: 'User rights' },
	{ path: '/wiki/Special:ProtectedPages', heading: 'Protected pages' },
	{ path: '/wiki/Special:Block', heading: 'Block an account' },
	{ path: '/w/index.php?title=Item:Q6&action=protect', heading: 'Protect Item:Q6' },
	{ path: '/w/index.php?title=Item:Q6&action=delete', heading: 'Delete Item:Q6' },
	{ path: '/wiki/Special:Log', heading: 'Logs' },
	{ path: '/wiki/Special:Log/delete', heading: 'Logs' },
	{ path: '/wiki/Special:BlockList', heading: 'Blocked accounts' },
	{ path: '/wiki/Special:Undelete', heading: 'Undelete' },
	{ path: '/wiki/Special:Preferences', heading: 'Preferences' },
	{ path: '/wiki/Special:BlankPage', heading: 'Blank page' },
	{ path: '/wiki/Special:EditPage', heading: 'Edit a page' },
	{ path: '/wiki/Special:Redirect', heading: 'Redirect by page or revision ID' },
	{ path: '/wiki/Special:PermanentLink/abc', heading: 'Permanent link', status: 400 },
	{ path: '/wiki/Special:NamespaceInfo', heading: 'Namespace information' },
	{ path: '/wiki/Special:ListGroupRights', heading: 'User group rights' },
	{ path: '/wiki/Special:ListDatatypes', heading: 'Data types' },
	{ path: '/wiki/Special:MyLanguageFallbackChain', heading: 'My language fallback chain' },
	{ path: '/wiki/Special:ProtectedTitles', heading: 'Protected titles' },
	{ path: '/wiki/Special:AllMessages', heading: 'All messages' },
	{ path: '/wiki/Special:ComparePages', heading: 'Compare pages' },
	{ path: '/w/index.php?title=Special:ComparePages&page1=Project:Home&page2=Item:Q6', heading: 'Compare pages', status: 404 },
	{ path: '/wiki/Special:AllPages', heading: 'All pages' },
	{ path: '/wiki/Special:PrefixIndex/Project:', heading: 'Pages with prefix' },
	{ path: '/w/index.php?title=Special:AllPages&namespace=120', heading: 'All pages' },
	{ path: '/wiki/Special:NewPages', heading: 'New pages' },
	{ path: '/wiki/Special:Providers', heading: 'Providers' },
	{ path: '/wiki/Special:SetAliases', heading: 'Set aliases' },
	{ path: '/wiki/Special:WantedPages', heading: 'Wanted pages' },
	{ path: '/wiki/Special:LonelyPages', heading: 'Orphaned pages' },
	{ path: '/wiki/Special:ShortPages?limit=20', heading: 'Short pages' },
	{ path: '/wiki/Special:ListRedirects', heading: 'List of redirects' },
	{ path: '/wiki/Special:ListProperties', heading: 'List of properties' },
	{ path: '/wiki/Special:WhatLinksHere', heading: 'Pages that link here' },
	{ path: '/wiki/Special:WhatLinksHere/Item:Q6', heading: 'Pages that link to "Item:Q6"' }
];

for ( const p of pages ) {
	test( `${ p.path } reads without JavaScript`, async ( { page } ) => {
		const response = await page.goto( p.path );
		expect( response.status() ).toBe( p.status || 200 );
		await expect( page.locator( 'html' ) ).toHaveClass( /client-nojs/ );
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( p.heading );
		expect( await overflow( page ) ).toBe( 0 );
	} );

	// axe runs as a script in the page, so this context allows scripts; the site's own
	// script only marks the page as having them.
	test( `${ p.path } passes axe`, async ( { browser } ) => {
		const context = await browser.newContext( { javaScriptEnabled: true } );
		const page = await context.newPage();
		await page.goto( p.path );
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

test( 'folds open and footnotes link to the references without JavaScript', async ( { page } ) => {
	await page.goto( '/wiki/Item:Q6' );
	const fold = page.locator( '#P2 details.ts-fold' ).first();
	await expect( fold.getByText( 'city' ) ).toBeHidden();
	await fold.locator( 'summary' ).click();
	await expect( fold.getByText( 'city' ) ).toBeVisible();
	// A footnote marker goes to the tab's references, which name the groups citing each.
	const marker = page.locator( '#P3 a.ts-fn' ).first();
	const n = await marker.textContent();
	await marker.click();
	await expect( page ).toHaveURL( ( url ) => url.hash === `#ts-ref-${ n }` );
	const note = page.locator( `#ts-ref-${ n }` );
	await expect( note.getByText( 'reference URL' ) ).toBeVisible();
	await expect( note.locator( '.ts-endnotes__cited a[href="#P3"]' ) ).toBeVisible();
} );

test( 'the header box goes to an ID and searches words without JavaScript', async ( { page } ) => {
	await page.goto( '/wiki/Item:Q6' );
	const box = page.locator( 'header input[name="search"]' );
	await box.fill( 'P3' );
	await box.press( 'Enter' );
	await expect( page ).toHaveURL( /\/wiki\/Property:P3$/ );
	await box.fill( 'six' );
	await box.press( 'Enter' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Search' );
	await expect( page.locator( '.ts-hit__title' ).first() ).toHaveText( 'Six & more' );
} );

test( 'the main page is Project:Home, and unbuilt features are not linked', async ( { page } ) => {
	await page.goto( '/' );
	await expect( page ).toHaveURL( /\/wiki\/Project:Home$/ );
	await page.goto( '/wiki/Main_Page' );
	await expect( page ).toHaveURL( /\/wiki\/Project:Home$/ );
	await expect( page.locator( 'a[href="/wiki/Special:NewItem"]' ) ).toHaveCount( 1 );
	await expect( page.locator( 'a[href="/wiki/Special:RecentChanges"]' ) ).toHaveCount( 1 );
	for ( const absent of [ 'Special:Jobs' ] ) {
		await expect( page.locator( `a[href*="${ absent }"]` ) ).toHaveCount( 0 );
	}
	await page.goto( '/wiki/Item:Q6' );
	await expect( page.locator( 'a[href*="action=history"]' ) ).toHaveCount( 1 );
} );

test( 'the history tab lists a mirrored domain\'s syncs and a local item\'s edits', async ( { page } ) => {
	await page.goto( '/wiki/Domain:wikipedia.org' );
	await page.getByRole( 'link', { name: 'History' } ).click();
	await expect( page ).toHaveURL( /action=history$/ );
	await expect( page.locator( '.ts-change--sync' ).first() ).toBeVisible();
	await expect( page.getByText( 'Only the latest state from' ) ).toBeVisible();
	// One source contributes, so there is no switch (chapter 19 §3.5).
	await expect( page.locator( '.ts-history__switch' ) ).toHaveCount( 0 );
	await page.goto( '/wiki/Item:Q6?action=history' );
	await expect( page.locator( '.ts-change--edit' ).first() ).toBeVisible();
	await expect( page.locator( '.ts-change--sync' ) ).toHaveCount( 0 );
	expect( await overflow( page ) ).toBe( 0 );
} );

test( 'a history row opens its diff, which reaches the old revision', async ( { page } ) => {
	await page.goto( '/wiki/Item:Q6?action=history' );
	await page.locator( '.ts-change__diff' ).first().click();
	await expect( page ).toHaveURL( /\?diff=\d+$/ );
	await expect( page.getByRole( 'heading', { level: 3, name: 'About this edit' } ) ).toBeVisible();
	expect( await overflow( page ) ).toBe( 0 );
	// The change-set view, and back.
	await page.getByRole( 'link', { name: 'Change set' } ).click();
	await expect( page.locator( '.ts-diff__changeset' ) ).toBeVisible();
	await page.locator( '.ts-diff__views' ).getByRole( 'link', { name: 'Statements' } ).click();
	// The revision's own page as it then was.
	await page.getByRole( 'link', { name: /^Revision \d+$/ } ).first().click();
	await expect( page ).toHaveURL( /\?oldid=\d+$/ );
	await expect( page.getByText( /^This is revision \d+ of this entity/ ) ).toBeVisible();
	await page.getByRole( 'link', { name: 'Current version' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item:Q6$/ );
} );

test( 'logging in and out works without JavaScript, through either replica', async ( { page } ) => {
	await page.goto( '/wiki/Item:Q6' );
	await page.getByRole( 'link', { name: 'Log in' } ).click();
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Log in' );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( 'not her password' );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
	await expect( page.getByText( 'Incorrect username or password.' ) ).toBeVisible();
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item:Q6$/ );
	await expect( page.locator( 'summary[aria-label="Account menu for Alice"]' ) ).toBeVisible();
	await page.locator( 'summary[aria-label="Account menu for Alice"]' ).click();
	await page.getByRole( 'link', { name: 'Log out' } ).click();
	await expect( page.getByText( 'Do you want to log out?' ) ).toBeVisible();
	await page.getByRole( 'button', { name: 'Log out' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item:Q6$/ );
	await expect( page.getByRole( 'link', { name: 'Log in' } ) ).toBeVisible();
} );

test( 'a phone reads an item without scrolling sideways', async ( { browser } ) => {
	const context = await browser.newContext( {
		viewport: { width: 390, height: 844 },
		javaScriptEnabled: false
	} );
	const page = await context.newPage();
	await page.goto( '/wiki/Item:Q6' );
	expect( await overflow( page ) ).toBe( 0 );
	await context.close();
} );

test( 'consecutive requests alternate replicas and get the same page', async ( { request } ) => {
	const a = await request.get( '/wiki/Item:Q6' );
	const b = await request.get( '/wiki/Item:Q6' );
	expect( a.headers()[ 'x-replica' ] ).not.toBe( b.headers()[ 'x-replica' ] );
	expect( await a.text() ).toBe( await b.text() );
	expect( a.headers().etag ).toBe( b.headers().etag );
	const again = await request.get( '/wiki/Item:Q6', { headers: { 'if-none-match': a.headers().etag } } );
	expect( again.status() ).toBe( 304 );
} );

test( 'an anonymous page is public; a signed-in page is private', async ( { playwright, baseURL } ) => {
	const anon = await playwright.request.newContext( { baseURL } );
	const page = await anon.get( '/wiki/Item:Q6' );
	expect( page.headers()[ 'cache-control' ] ).toMatch( /^public/ );
	expect( page.headers()[ 'cache-tag' ] ).toContain( 'entity:Q6' );
	await anon.dispose();

	const alice = await playwright.request.newContext( { baseURL } );
	const tokens = await ( await alice.get(
		'/w/api.php?action=query&meta=tokens&type=login&format=json&formatversion=2'
	) ).json();
	const login = await ( await alice.post( '/w/api.php', {
		form: {
			action: 'clientlogin',
			format: 'json',
			formatversion: '2',
			username: 'Alice',
			password: process.env.E2E_OWNER_PASSWORD,
			loginreturnurl: `${ baseURL }/wiki/Project:Home`,
			logintoken: tokens.query.tokens.logintoken
		}
	} ) ).json();
	expect( login.clientlogin.status ).toBe( 'PASS' );
	for ( let i = 0; i < 2; i++ ) {
		// Both replicas know her: the session lives in the API, not in either replica.
		const mine = await alice.get( '/wiki/Item:Q6' );
		expect( mine.headers()[ 'cache-control' ] ).toBe( 'private, no-cache' );
		expect( mine.headers()[ 'cache-tag' ] ).toBeUndefined();
		expect( await mine.text() ).toContain( 'Account menu for Alice' );
	}
	await alice.dispose();
} );

test( 'recent changes list the adoption, filter without JavaScript, and reach contributions', async ( { page } ) => {
	await page.goto( '/wiki/Special:RecentChanges' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Recent changes' );
	// The adopted entities' first records are edits made here; mirror syncs are off.
	await expect( page.locator( '.ts-change--edit' ).first() ).toBeVisible();
	await expect( page.locator( '.ts-change--sync' ) ).toHaveCount( 0 );
	await expect( page.getByText( 'Mirror syncs are not shown.' ) ).toBeVisible();
	// The strip's link turns them on.
	await page.getByRole( 'link', { name: 'Show them' } ).click();
	await expect( page.locator( '.ts-change--sync' ).first() ).toBeVisible();
	await expect( page.getByLabel( 'Mirror syncs' ) ).toBeChecked();
	// The form narrows to properties.
	await page.getByLabel( 'Namespace' ).selectOption( '122' );
	await page.getByRole( 'button', { name: 'Show changes' } ).click();
	await expect( page.locator( '.ts-change__target' ).first() ).toHaveAttribute( 'href', /Property:/ );
	// An actor's name leads to their contributions.
	await page.getByRole( 'link', { name: 'Alice', exact: true } ).first().click();
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Contributions of Alice' );
	await expect( page.locator( '.ts-change' ).first() ).toBeVisible();
	expect( await overflow( page ) ).toBe( 0 );
} );

// ADR 0034 A13: `style` attributes apply, which wikitext writes and the Sanitizer has
// checked; `<style>` elements stay refused.
test( 'style attributes apply under the security policy; style elements do not', async ( { browser } ) => {
	const context = await browser.newContext( { javaScriptEnabled: true } );
	const page = await context.newPage();
	await page.goto( '/wiki/Special:SpecialPages' );
	const colours = await page.evaluate( () => {
		const attr = document.createElement( 'span' );
		attr.setAttribute( 'style', 'color: rgb(1, 2, 3)' );
		attr.textContent = 'x';
		const sheet = document.createElement( 'style' );
		sheet.textContent = '.ts-csp-probe { color: rgb(4, 5, 6) }';
		const el = document.createElement( 'span' );
		el.className = 'ts-csp-probe';
		el.textContent = 'y';
		document.head.append( sheet );
		document.body.append( attr, el );
		return [ getComputedStyle( attr ).color, getComputedStyle( el ).color ];
	} );
	expect( colours[ 0 ] ).toBe( 'rgb(1, 2, 3)' );
	expect( colours[ 1 ] ).not.toBe( 'rgb(4, 5, 6)' );
	await context.close();
} );

test( 'recent changes mark accounts and show the instance as the Server Operator', async ( { page } ) => {
	// ADR 0095 §3: the instance's own key is a badge, an account target an icon and a
	// name linked to its contributions, and no key for this wiki's own accounts.
	await page.goto( '/wiki/Special:RecentChanges' );
	const badge = page.locator( '.ts-chip--operator' ).first();
	await expect( badge ).toHaveText( 'Server Operator' );
	await expect( badge ).toHaveAttribute( 'title', /^instance:/ );
	const account = page.locator( '.ts-change__account' ).first();
	await expect( account.locator( '.ts-change__account-icon' ) ).toHaveCount( 1 );
	await expect( account.locator( 'a' ) ).toHaveAttribute( 'href', /\/wiki\/Special:Contributions\// );
	await expect( page.locator( '.ts-change--log .ts-change__id' ) ).toHaveCount( 0 );
	// Entities keep their IDs.
	await expect( page.locator( '.ts-change__id', { hasText: /^Q6$/ } ).first() ).toBeVisible();
} );

/**
 * Logs Alice in through the site's own form.
 *
 * @param {Object} page A Playwright page
 */
async function logIn( page ) {
	await page.goto( '/wiki/Special:UserLogin?returnto=Project:Home' );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
}

// A document page's tabs and panels (chapter 19 §1.3, §3.1; ADR 0010 §4, §12, 0019 §2):
// Read, Edit, History, Links here and Discussion, and the About this page panel; the Format
// selector of a new page (0041 §10). Serial: the second creates what the first lists.
test.describe.serial( 'a document page', () => {
	test( 'has Links here and Discussion tabs and an About this page panel', async ( { page } ) => {
		await logIn( page );
		await page.goto( '/w/index.php?title=Project:Tabs_test&action=edit' );
		await page.locator( '#wpTextbox1' ).fill( 'See [[Project:Tab target]] and [[Item:Q6]].' );
		await page.locator( 'button[name="wpSave"]' ).click();
		await expect( page ).toHaveURL( /\/wiki\/(Project|librarybase):Tabs_test$/ );
		const tabs = page.locator( 'nav.ts-tabs a.ts-tabs__link' );
		await expect( tabs ).toHaveText( [ 'Read', 'Edit', 'History', 'Links here', 'Discussion' ] );
		await expect( tabs.first() ).toHaveAttribute( 'aria-current', 'page' );

		// About this page: who made it and when, its size, model and protection.
		const about = page.getByRole( 'complementary', { name: 'About this page' } );
		await expect( about ).toBeVisible();
		await expect( about.locator( '.ts-about__fact' ).filter( { hasText: 'Created' } ) ).toContainText( 'Alice' );
		await expect( about.locator( '.ts-about__fact' ).filter( { hasText: 'Content model' } ) ).toContainText( 'Wikitext' );
		await expect( about.locator( '.ts-about__fact' ).filter( { hasText: 'Protection' } ) ).toContainText( 'Not protected' );
		await expect( about.getByRole( 'link', { name: '(diff)' } ) ).toHaveAttribute( 'href', /diff=\d+$/ );

		// Discussion is the talk page, which offers to start a thread.
		await page.getByRole( 'link', { name: 'Discussion' } ).click();
		await expect( page ).toHaveURL( /\/wiki\/Project_talk:Tabs_test$/ );
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( /^(Project|librarybase) talk:Tabs test$/ );
		await expect( page.getByRole( 'link', { name: 'Add topic' } ) ).toBeVisible();
		await page.getByRole( 'link', { name: 'Page', exact: true } ).click();
		await expect( page ).toHaveURL( /\/wiki\/(Project|librarybase):Tabs_test$/ );

		// Links here, on a free title: its wanted links. And on an item, by its ID.
		await page.goto( '/wiki/Project:Tab_target' );
		await page.getByRole( 'link', { name: 'Links here' } ).click();
		await expect( page ).toHaveURL( /\/wiki\/Special:WhatLinksHere\/Project:Tab_target$/ );
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Pages that link to "Project:Tab target"' );
		await expect( page.locator( 'ul.ts-whatlinkshere' ).getByRole( 'link', { name: /^(Project|librarybase):Tabs test$/ } ) ).toBeVisible();
		await page.goto( '/wiki/Item:Q6' );
		await page.getByRole( 'link', { name: 'Links here' } ).click();
		await expect( page.locator( 'ul.ts-whatlinkshere' ).getByRole( 'link', { name: /^(Project|librarybase):Tabs test$/ } ) ).toBeVisible();
		await page.goto( '/wiki/Special:WhatLinksHere/Project:Nothing_links_here' );
		await expect( page.getByText( 'No pages link to Project:Nothing links here.' ) ).toBeVisible();
	} );

	test( 'is created in the format chosen, and edited in it', async ( { page } ) => {
		await logIn( page );
		await page.goto( '/w/index.php?title=Project:Markdown_note&action=edit' );
		const format = page.getByLabel( 'Format' );
		await expect( format ).toHaveValue( 'wikitext' );
		await format.selectOption( 'markdown' );
		await page.locator( '#wpTextbox1' ).fill( '# A note\n\nWritten in *Markdown*.' );
		await page.locator( 'button[name="wpPreview"]' ).click();
		// The preview keeps the choice.
		await expect( page.getByLabel( 'Format' ) ).toHaveValue( 'markdown' );
		await page.locator( 'button[name="wpSave"]' ).click();
		await expect( page ).toHaveURL( /\/wiki\/(Project|librarybase):Markdown_note$/ );
		const about = page.getByRole( 'complementary', { name: 'About this page' } );
		await expect( about.locator( '.ts-about__fact' ).filter( { hasText: 'Content model' } ) ).toContainText( 'Markdown' );
		// A page that exists has no selector, and the form edits its text.
		await page.getByRole( 'link', { name: 'Edit', exact: true } ).click();
		await expect( page.locator( '#wpTextbox1' ) ).toHaveValue( /Written in \*Markdown\*/ );
		await expect( page.getByLabel( 'Format' ) ).toHaveCount( 0 );
	} );

	test( 'passes axe with its panel, and on its talk page', async ( { browser } ) => {
		const context = await browser.newContext( { javaScriptEnabled: true } );
		const page = await context.newPage();
		for ( const path of [ '/wiki/Project:Tabs_test', '/wiki/Project_talk:Tabs_test', '/wiki/Special:WhatLinksHere/Project:Tab_target' ] ) {
			await page.goto( path );
			const results = await new AxeBuilder( { page } )
				.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
				.analyze();
			expect( results.violations.map( ( v ) => `${ path } ${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }` ) ).toEqual( [] );
		}
		await context.close();
	} );
} );
