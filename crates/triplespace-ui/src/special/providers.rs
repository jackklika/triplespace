//! `Special:Providers` (0022 §11; 19 §5): the data providers this instance knows, from
//! `siprop=providers`: each one's chip and name, the graph its mirror is written to, and
//! for each entity type it mints, the prefix its IDs carry here, the namespace their pages
//! are in and the concept IRI they stand for.
//!
//! The live columns 19 §5 also gives the page (trust mode, last verified checkpoint, set
//! size, queue and lag, and the page repositories) read `GET /provider/{code}/mirror` and
//! `GET /repo/{name}` (18 §3.2), which the API does not serve yet.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::ProviderInfo;

use crate::Site;
use crate::codex::{Chip, ChipKind};
use crate::frame::Page;
use crate::i18n::Messages;
use crate::pages::{self, Params, Peer};

const TITLE: &str = "Special:Providers";

/// `GET Special:Providers`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let model = model(&cx.m, &cx.site.providers);
    let body = cx.draw("special/providers.html", &model);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-providers-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

/// `Special:Providers` as Plain's `special/providers.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProvidersPage {
    /// What the page lists.
    pub intro: String,
    /// Said where there are none.
    pub nothing: Option<String>,
    /// The column headings.
    pub columns: Vec<String>,
    /// One row per provider.
    pub rows: Vec<ProviderRow>,
}

/// One provider.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderRow {
    /// Its chip.
    pub chip: crate::codex::ChipModel,
    /// Its name.
    pub name: String,
    /// Its mirror's graph's slug.
    pub slug: String,
    /// One line per entity type it mints.
    pub types: Vec<String>,
}

fn model(m: &Messages, providers: &[ProviderInfo]) -> ProvidersPage {
    ProvidersPage {
        intro: m.get("ts-providers-intro"),
        nothing: providers.is_empty().then(|| m.get("ts-providers-none")),
        columns: [
            "ts-providers-col-provider",
            "ts-providers-col-graph",
            "ts-providers-col-types",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rows: providers
            .iter()
            .map(|p| ProviderRow {
                chip: Chip::new(ChipKind::Provider(p.code.clone()), &p.code).model(),
                name: p.name.clone(),
                slug: p.slug.clone(),
                types: p
                    .types
                    .iter()
                    .map(|t| {
                        m.with(
                            "ts-providers-type",
                            &[
                                &t.entity_type,
                                &format!("{}{}", p.code, t.code),
                                t.namespace.as_deref().unwrap_or_default(),
                                &t.iri,
                            ],
                        )
                    })
                    .collect(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::ProviderType;

    #[test]
    fn each_type_names_its_prefix_namespace_and_iri() {
        let m = Messages::for_language("en");
        let wd = ProviderInfo {
            code: "WD".into(),
            slug: "wikidata".into(),
            name: "Wikidata".into(),
            types: vec![ProviderType {
                code: "Q".into(),
                entity_type: "item".into(),
                iri: "http://www.wikidata.org/entity/{upstream_id}".into(),
                upstream_prefix: "Q".into(),
                namespace: Some("Item".into()),
            }],
            chip: None,
        };
        let plain = crate::skin::Skins::load(None);
        let plain = plain.get("plain").expect("plain");
        let body = |p: &[ProviderInfo]| {
            plain
                .render("special/providers.html", &m, model(&m, p))
                .unwrap()
        };
        let html = body(&[wd]);
        assert!(
            html.contains("Wikidata</td><td><code>mirror/wikidata</code>"),
            "{html}"
        );
        assert!(html.contains("WDQ"), "{html}");
        assert!(html.contains("Item"), "{html}");
        assert!(html.contains("http://www.wikidata.org/entity/{upstream_id}"));
        assert!(body(&[]).contains("ts-empty"));
    }
}
