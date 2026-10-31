//! What the entity page's models read of the entity: its title and labels, data types
//! and property order. The page's parts themselves are [`super::model`] and the skin's
//! `entity/` templates since groups 3 and 4 of the skin migration switched (19 §5.10).

use std::collections::BTreeMap;

use scatter_wikibase_model::id::IdForm;
use scatter_wikibase_model::statement::Statement;

use super::Render;

impl Render<'_> {
    pub(super) fn pick<'m>(&self, map: &'m BTreeMap<String, String>) -> Option<(&'m str, &'m str)> {
        self.m
            .term_chain()
            .into_iter()
            .find_map(|l| map.get_key_value(&l))
            .map(|(l, t)| (t.as_str(), l.as_str()))
    }

    /// The label in the reader's term chain, with its language.
    #[must_use]
    pub fn label(&self) -> Option<(&str, &str)> {
        self.pick(&self.entity.labels)
    }

    /// The title as shown: the label, else the ID, or for a keyed entity its key.
    #[must_use]
    pub fn display_title(&self) -> String {
        if let Some((t, _)) = self.label() {
            return t.to_string();
        }
        match self.entity.id.keyed_parts() {
            Some((_, key)) => key.to_string(),
            None => self.entity.id.as_str().to_string(),
        }
    }

    /// The document title: the label with the ID, as Wikibase shows it.
    #[must_use]
    pub fn document_title(&self) -> String {
        match self.label() {
            Some((t, _)) if self.entity.id.form() != IdForm::Keyed => {
                format!("{t} ({})", self.entity.id.as_str())
            }
            Some((t, _)) => t.to_string(),
            None => self.display_title(),
        }
    }

    pub(super) fn provider_name(&self, code: &str) -> String {
        self.site
            .providers
            .iter()
            .find(|p| p.code == code)
            .map_or_else(|| code.to_string(), |p| p.name.clone())
    }

    pub(super) fn concept_iri(&self) -> Option<String> {
        let id = &self.entity.id;
        match id.form() {
            IdForm::Local if !self.site.concept_base.is_empty() => {
                Some(format!("{}{}", self.site.concept_base, id.as_str()))
            }
            IdForm::Foreign => {
                let s = id.as_str();
                let (code, rest) = (&s[..2], &s[2..]);
                let type_code = rest.chars().next()?.to_string();
                let p = self.site.providers.iter().find(|p| p.code == code)?;
                let t = p.types.iter().find(|t| t.code == type_code)?;
                let upstream = self
                    .provenance
                    .and_then(|pr| pr.minted_by.upstream_id.clone())
                    .unwrap_or_else(|| format!("{}{}", t.upstream_prefix, &rest[1..]));
                Some(t.iri.replace("{upstream_id}", &upstream))
            }
            _ => None,
        }
    }

    pub(super) fn datatype_name(&self, id: &str) -> String {
        let key = format!("ts-datatype-{}", id.to_ascii_lowercase());
        let msg = self.m.get(&key);
        if msg.starts_with('⧼') {
            id.to_string()
        } else {
            msg
        }
    }

    /// A statement's property's data type: from its main snak, else from the lookup.
    pub(super) fn datatype_of(&self, property: &str, statements: &[Statement]) -> Option<String> {
        statements
            .iter()
            .find_map(|s| s.mainsnak.datatype.as_ref().map(|d| d.id().to_string()))
            .or_else(|| self.lookup.datatype(property).map(str::to_string))
    }

    pub(super) fn is_identifier(&self, property: &str, statements: &[Statement]) -> bool {
        self.datatype_of(property, statements).as_deref() == Some("external-id")
    }

    /// The properties of one tab, in property order: by provider, then by number, so
    /// that `P2` comes before `P10` (the API serves them in canonical, textual order).
    pub(super) fn tab_properties(&self, identifiers: bool) -> Vec<&str> {
        let mut props: Vec<&str> = self
            .entity
            .statements
            .iter()
            .filter(|(p, list)| self.is_identifier(p.as_str(), list) == identifiers)
            .map(|(p, _)| p.as_str())
            .collect();
        props.sort_by_key(|p| property_order(p));
        props
    }
}

/// The sort key of a property ID: its prefix (empty for a local ID), then its number.
fn property_order(id: &str) -> (String, u64, String) {
    let digits = id.trim_start_matches(|c: char| !c.is_ascii_digit());
    let prefix = &id[..id.len() - digits.len()];
    let prefix = prefix.strip_suffix('P').unwrap_or(prefix);
    (
        prefix.to_string(),
        digits.parse().unwrap_or(u64::MAX),
        id.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::property_order;

    #[test]
    fn properties_sort_by_number() {
        let mut v = vec!["P10", "WDP31", "P2", "P12", "WDP5"];
        v.sort_by_key(|p| property_order(p));
        assert_eq!(v, vec!["P2", "P10", "P12", "WDP5", "WDP31"]);
    }
}
