//! IRI constants for the `scatter:` vocabulary and for the external vocabularies the ADRs
//! reuse (ADR 0005 §2 and §5; 0001 §6; 0007 §9; 0011 §8).
//!
//! This crate is the only place `scatter:` terms are defined (0005 §5). Both Triplespace
//! and Scatterbase take their term IRIs from here. Terms are minted under
//! `https://scatter.red/terms/v0/`; `v0` makes no stability promise yet.
//!
//! Every constant is an [`oxrdf::NamedNodeRef`], so the terms can be used in RDF directly
//! and no crate defines its own term types (0005 §3, rule 5). Each vocabulary module also
//! exposes its namespace IRI as `NS` and a `term` function for terms not listed here.
//!
//! Instance data — revision, user, graph and entity IRIs — is *not* here. Those belong to
//! each instance's base URI (0001 §5) and are built by the surfaces.

/// Declares a vocabulary module: its namespace and a list of terms.
macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        $name:ident, $ns:literal, $prefix:literal {
            $( $(#[$tmeta:meta])* $term:ident = $local:literal ; )*
        }
    ) => {
        $(#[$meta])*
        pub mod $name {
            use oxrdf::NamedNodeRef;

            /// The namespace IRI.
            pub const NS: &str = $ns;
            /// The conventional prefix.
            pub const PREFIX: &str = $prefix;

            $(
                #[doc = concat!("The term `", $prefix, ":", $local, "`.")]
                $(#[$tmeta])*
                pub const $term: NamedNodeRef<'static> =
                    NamedNodeRef::new_unchecked(concat!($ns, $local));
            )*

            /// Every term this module defines, with its local name.
            pub const TERMS: &[(&str, NamedNodeRef<'static>)] = &[
                $( ($local, $term), )*
            ];
        }
    };
}

vocabulary! {
    /// Triplespace's and Scatterbase's own terms (0001 §5–6 as amended by 0005 §5, 0007 §9,
    /// 0011 §8, 0022).
    scatter, "https://scatter.red/terms/v0/", "scatter" {
        // Revisions (0001 §6)
        /// A revision of an entity or page on this instance.
        REVISION = "Revision";
        /// The parsed edit-summary operation, such as `wbsetlabel-add`.
        SUMMARY_OPERATION = "summaryOperation";
        /// The parsed edit-summary arguments, as MediaWiki joins them.
        SUMMARY_ARGS = "summaryArgs";
        /// The full comment text.
        COMMENT = "comment";
        /// The minor flag.
        MINOR = "minor";
        /// Size in bytes after the change.
        SIZE = "size";
        /// SHA-1 of the content, as MediaWiki reports it.
        SHA1 = "sha1";
        /// The content model of the revision.
        CONTENT_MODEL = "contentModel";
        /// The log offset a triple was added in (0001 §2).
        ADDED_IN = "addedIn";
        /// A revision whose actor is hidden (0007 §9).
        ATTRIBUTION_HIDDEN = "attributionHidden";

        // Actors (0007 §9)
        /// A temporary account, a subclass of `prov:Agent`.
        TEMPORARY_ACCOUNT = "TemporaryAccount";
        /// An anonymous (IP) surrogate actor.
        ANONYMOUS_ACTOR = "AnonymousActor";
        /// An imported-edit surrogate actor.
        IMPORTED_ACTOR = "ImportedActor";
        /// The issuer an actor belongs to (0007, open question).
        ISSUED_BY = "issuedBy";

        // Log events (0011 §8)
        /// A log event's MediaWiki type.
        LOG_TYPE = "logType";
        /// A log event's MediaWiki action.
        LOG_ACTION = "logAction";
        /// A job's log event.
        JOB_EVENT = "JobEvent";
        /// Erasure of a record or part.
        ERASE = "Erase";
        /// A rename.
        RENAME = "Rename";
        /// A protection change.
        PROTECT = "Protect";
        /// A change of revision visibility.
        CHANGE_VISIBILITY = "ChangeVisibility";
        /// An import.
        IMPORT = "Import";
        /// Setting a retention policy (0002 §5).
        SET_RETENTION = "SetRetention";
        /// Converting a foreign entity to a local one (0002 §6).
        CONVERT = "Convert";
        /// Linking an account (0007 §7).
        LINK_ACCOUNT = "LinkAccount";

        // Federation (0022)
        /// The checkpoint a mirrored batch was verified against.
        VERIFIED_AGAINST = "verifiedAgainst";
    }
}

vocabulary! {
    /// W3C PROV-O.
    prov, "http://www.w3.org/ns/prov#", "prov" {
        ENTITY = "Entity";
        ACTIVITY = "Activity";
        AGENT = "Agent";
        ORGANIZATION = "Organization";
        SOFTWARE_AGENT = "SoftwareAgent";
        WAS_REVISION_OF = "wasRevisionOf";
        SPECIALIZATION_OF = "specializationOf";
        GENERATED_AT_TIME = "generatedAtTime";
        INVALIDATED_AT_TIME = "invalidatedAtTime";
        WAS_ATTRIBUTED_TO = "wasAttributedTo";
        WAS_GENERATED_BY = "wasGeneratedBy";
        WAS_DERIVED_FROM = "wasDerivedFrom";
        ACTED_ON_BEHALF_OF = "actedOnBehalfOf";
    }
}

vocabulary! {
    /// PAV, the Provenance, Authoring and Versioning ontology.
    pav, "http://purl.org/pav/", "pav" {
        VERSION = "version";
        PREVIOUS_VERSION = "previousVersion";
        HAS_CURRENT_VERSION = "hasCurrentVersion";
        IMPORTED_FROM = "importedFrom";
        RETRIEVED_FROM = "retrievedFrom";
        IMPORTED_ON = "importedOn";
    }
}

vocabulary! {
    /// SIOC core.
    sioc, "http://rdfs.org/sioc/ns#", "sioc" {
        USER_ACCOUNT = "UserAccount";
        NAME = "name";
        ACCOUNT_OF = "account_of";
        HAS_CREATOR = "has_creator";
        HAS_MODIFIER = "has_modifier";
    }
}

vocabulary! {
    /// FOAF.
    foaf, "http://xmlns.com/foaf/0.1/", "foaf" {
        AGENT = "Agent";
    }
}

vocabulary! {
    /// Activity Streams 2.0.
    activitystreams, "https://www.w3.org/ns/activitystreams#", "as" {
        CREATE = "Create";
        UPDATE = "Update";
        DELETE = "Delete";
        MOVE = "Move";
        ANNOUNCE = "Announce";
        TOMBSTONE = "Tombstone";
        PERSON = "Person";
        GROUP = "Group";
        COLLECTION = "Collection";
        ACTOR = "actor";
        OBJECT = "object";
        PUBLISHED = "published";
    }
}

vocabulary! {
    /// Dublin Core Terms.
    dcterms, "http://purl.org/dc/terms/", "dcterms" {
        IS_VERSION_OF = "isVersionOf";
        REPLACES = "replaces";
        IS_REPLACED_BY = "isReplacedBy";
    }
}

vocabulary! {
    /// schema.org.
    schema, "http://schema.org/", "schema" {
        VERSION = "version";
        ABOUT = "about";
        DATASET = "Dataset";
        DATE_MODIFIED = "dateModified";
        NAME = "name";
        DESCRIPTION = "description";
    }
}

vocabulary! {
    /// The Wikibase ontology (`docs/samples/wikibase-1.43/wikibase-ontology-1.0.owl`).
    wikibase, "http://wikiba.se/ontology#", "wikibase" {
        ITEM = "Item";
        PROPERTY = "Property";
        STATEMENT = "Statement";
        REFERENCE = "Reference";
        RANK = "rank";
        NORMAL_RANK = "NormalRank";
        PREFERRED_RANK = "PreferredRank";
        DEPRECATED_RANK = "DeprecatedRank";
        BEST_RANK = "BestRank";
        TIME_VALUE = "TimeValue";
        QUANTITY_VALUE = "QuantityValue";
        GLOBECOORDINATE_VALUE = "GlobecoordinateValue";
        SITELINKS = "sitelinks";
        STATEMENTS = "statements";
        IDENTIFIERS = "identifiers";
        PROPERTY_TYPE = "propertyType";
        DIRECT_CLAIM = "directClaim";
        CLAIM = "claim";
        STATEMENT_PROPERTY = "statementProperty";
        STATEMENT_VALUE = "statementValue";
        QUALIFIER = "qualifier";
        QUALIFIER_VALUE = "qualifierValue";
        REFERENCE_PROPERTY = "reference";
        REFERENCE_VALUE = "referenceValue";
        NOVALUE = "novalue";
        BADGE = "badge";
        WIKI_GROUP = "wikiGroup";
    }
}

vocabulary! {
    /// RDF.
    rdf, "http://www.w3.org/1999/02/22-rdf-syntax-ns#", "rdf" {
        TYPE = "type";
    }
}

vocabulary! {
    /// RDF Schema.
    rdfs, "http://www.w3.org/2000/01/rdf-schema#", "rdfs" {
        LABEL = "label";
    }
}

vocabulary! {
    /// OWL.
    owl, "http://www.w3.org/2002/07/owl#", "owl" {
        SAME_AS = "sameAs";
    }
}

vocabulary! {
    /// SKOS.
    skos, "http://www.w3.org/2004/02/skos/core#", "skos" {
        PREF_LABEL = "prefLabel";
        ALT_LABEL = "altLabel";
        /// A notation's string, typed by its scheme (0048 §5).
        NOTATION = "notation";
        /// The scheme a notation belongs to (0048 §5).
        IN_SCHEME = "inScheme";
    }
}

/// Builds a term in a namespace that is not listed above, checking the result is an IRI.
///
/// ```
/// let t = scatter_vocab::term(scatter_vocab::scatter::NS, "hashMismatches").unwrap();
/// assert_eq!(t.as_str(), "https://scatter.red/terms/v0/hashMismatches");
/// ```
pub fn term(ns: &str, local: &str) -> Result<oxrdf::NamedNode, oxrdf::IriParseError> {
    let mut iri = String::with_capacity(ns.len() + local.len());
    iri.push_str(ns);
    iri.push_str(local);
    oxrdf::NamedNode::new(iri)
}

/// Returns the conventional prefix for a namespace IRI, for serializers.
#[must_use]
pub fn prefix_for(ns: &str) -> Option<&'static str> {
    [
        (scatter::NS, scatter::PREFIX),
        (prov::NS, prov::PREFIX),
        (pav::NS, pav::PREFIX),
        (sioc::NS, sioc::PREFIX),
        (foaf::NS, foaf::PREFIX),
        (activitystreams::NS, activitystreams::PREFIX),
        (dcterms::NS, dcterms::PREFIX),
        (schema::NS, schema::PREFIX),
        (wikibase::NS, wikibase::PREFIX),
        (rdf::NS, rdf::PREFIX),
        (rdfs::NS, rdfs::PREFIX),
        (owl::NS, owl::PREFIX),
        (skos::NS, skos::PREFIX),
    ]
    .into_iter()
    .find(|(n, _)| *n == ns)
    .map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxrdf::{NamedNode, NamedNodeRef};

    #[test]
    fn scatter_terms_are_under_the_v0_namespace() {
        assert_eq!(scatter::NS, "https://scatter.red/terms/v0/");
        assert_eq!(
            scatter::REVISION.as_str(),
            "https://scatter.red/terms/v0/Revision"
        );
        assert_eq!(
            scatter::ADDED_IN.as_str(),
            "https://scatter.red/terms/v0/addedIn"
        );
    }

    #[test]
    fn every_constant_is_a_valid_iri_in_its_namespace() {
        let all: &[(&str, &[(&str, NamedNodeRef<'static>)])] = &[
            (scatter::NS, scatter::TERMS),
            (prov::NS, prov::TERMS),
            (pav::NS, pav::TERMS),
            (sioc::NS, sioc::TERMS),
            (foaf::NS, foaf::TERMS),
            (activitystreams::NS, activitystreams::TERMS),
            (dcterms::NS, dcterms::TERMS),
            (schema::NS, schema::TERMS),
            (wikibase::NS, wikibase::TERMS),
            (rdf::NS, rdf::TERMS),
            (rdfs::NS, rdfs::TERMS),
            (owl::NS, owl::TERMS),
            (skos::NS, skos::TERMS),
        ];
        for (ns, terms) in all {
            assert!(!terms.is_empty());
            for (local, node) in *terms {
                assert!(
                    node.as_str().starts_with(ns),
                    "{} not under {ns}",
                    node.as_str()
                );
                assert_eq!(&node.as_str()[ns.len()..], *local);
                // NamedNodeRef::new_unchecked skipped validation; check it now.
                NamedNode::new(node.as_str()).expect("valid IRI");
            }
        }
    }

    #[test]
    fn no_term_is_defined_twice_in_a_namespace() {
        for terms in [scatter::TERMS, prov::TERMS, wikibase::TERMS] {
            let mut seen = std::collections::BTreeSet::new();
            for (local, _) in terms {
                assert!(seen.insert(*local), "duplicate term {local}");
            }
        }
    }

    #[test]
    fn prefixes() {
        assert_eq!(prefix_for(scatter::NS), Some("scatter"));
        assert_eq!(prefix_for(activitystreams::NS), Some("as"));
        assert_eq!(prefix_for("http://example.org/"), None);
    }
}
