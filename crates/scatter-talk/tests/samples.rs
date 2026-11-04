//! Golden tests against DiscussionTools (0090 §13).
//!
//! They read `docs/samples/enwiki-talk/`: the 2026-10-21 samples `tools/talk_dumps.py` fetched from
//! English Wikipedia, compressed by `tools/talk_fixtures.py` (see the README there for the licence). Set
//! `TRIPLESPACE_TALK_SAMPLES` to a directory `tools/talk_dumps.py` wrote to run them on other
//! samples instead; each file is read as named, or with `.gz` or `.bz2` appended.
//!
//! The thresholds are the Python prototype's agreement on the 2026-10-21 samples
//! (`tools/talk_compare.py`): the Rust splitter is held to it or better.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::Event;
use scatter_talk::{
    Blame, Markup, Options, PostKind, Revision, Site, Stamp, Thread, attribute, convert,
    normalize_user,
};
use serde_json::Value;

fn samples() -> PathBuf {
    std::env::var_os("TRIPLESPACE_TALK_SAMPLES").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/samples/enwiki-talk"),
        PathBuf::from,
    )
}

/// A sample file's bytes: `name` as is, or compressed as `name.gz` or `name.bz2`.
fn read_sample(dir: &Path, name: &str) -> Vec<u8> {
    let mut raw = Vec::new();
    let plain = dir.join(name);
    let name = name.strip_suffix(".gz").unwrap_or(name);
    if plain.exists() && plain.extension().is_none_or(|e| e != "gz") {
        std::fs::File::open(&plain)
            .and_then(|mut f| f.read_to_end(&mut raw))
            .expect("sample reads");
    } else if let Ok(f) = std::fs::File::open(dir.join(format!("{name}.gz"))) {
        flate2::read::GzDecoder::new(f)
            .read_to_end(&mut raw)
            .expect("gzip");
    } else {
        let f = std::fs::File::open(dir.join(format!("{name}.bz2")))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        bzip2::read::BzDecoder::new(f)
            .read_to_end(&mut raw)
            .expect("bzip2");
    }
    raw
}

/// One revision of a dump: (title, revision ID, actor, timestamp, text).
type Rev = (String, u64, String, String, String);

fn read_dump(dir: &Path, name: &str) -> Vec<Rev> {
    let raw = read_sample(dir, name);
    let mut reader = Reader::from_reader(raw.as_slice());
    let mut out = Vec::new();
    let (mut title, mut stack, mut buf, mut leaf) = (
        String::new(),
        Vec::<String>::new(),
        Vec::new(),
        String::new(),
    );
    let mut cur: (u64, String, String, String) = Default::default();
    loop {
        match reader.read_event_into(&mut buf).expect("xml") {
            Event::Start(e) => {
                stack.push(String::from_utf8_lossy(e.name().as_ref()).into_owned());
                leaf.clear();
            }
            Event::Text(t) => leaf.push_str(&t.decode().expect("text")),
            Event::GeneralRef(r) => match r.resolve_char_ref().expect("ref") {
                Some(c) => leaf.push(c),
                None => leaf.push_str(match r.decode().expect("ref").as_ref() {
                    "lt" => "<",
                    "gt" => ">",
                    "amp" => "&",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                }),
            },
            Event::End(e) => {
                let path: Vec<&str> = stack.iter().map(String::as_str).collect();
                let v = std::mem::take(&mut leaf);
                match path.as_slice() {
                    [.., "page", "title"] => title = v,
                    [.., "revision", "id"] => cur.0 = v.parse().expect("revid"),
                    [.., "contributor", "username" | "ip"] => cur.1 = v,
                    [.., "revision", "timestamp"] => cur.2 = v,
                    [.., "revision", "text"] => cur.3 = v,
                    _ => {}
                }
                if e.name().as_ref() == b"revision" {
                    out.push((
                        title.clone(),
                        cur.0,
                        std::mem::take(&mut cur.1),
                        std::mem::take(&mut cur.2),
                        std::mem::take(&mut cur.3),
                    ));
                }
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn stamp(iso: &str) -> Stamp {
    let n = |a: usize, b: usize| iso[a..b].parse::<i64>().expect("number");
    Stamp::from_civil(n(0, 4), n(5, 7), n(8, 10), n(11, 13), n(14, 16)).expect("a date")
}

type Key = (String, Stamp);

/// Collects DiscussionTools' comments under `it`, each once.
fn walk(
    it: &Value,
    parent: Option<Key>,
    seen: &mut std::collections::HashSet<String>,
    out: &mut Vec<(Key, Option<Key>, String)>,
) {
    let id = it["id"].as_str().unwrap_or_default().to_string();
    if !seen.insert(id.clone()) {
        return;
    }
    let mut key = None;
    if it["type"] == "comment" {
        let k = (
            normalize_user(it["author"].as_str().unwrap_or_default()),
            stamp(it["timestamp"].as_str().unwrap_or_default()),
        );
        out.push((k.clone(), parent, id));
        key = Some(k);
    }
    for r in it["replies"].as_array().into_iter().flatten() {
        walk(r, key.clone(), seen, out);
    }
}

/// DiscussionTools' comments, once each: key, parent key, ID.
fn dt_comments(dir: &Path, name: &str) -> Vec<(Key, Option<Key>, String)> {
    let v: Value = serde_json::from_slice(&read_sample(dir, name)).expect("json");
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for it in v["discussiontoolspageinfo"]["threaditemshtml"]
        .as_array()
        .expect("items")
    {
        walk(it, None, &mut seen, &mut out);
    }
    out
}

/// Our dated comments: key → (parent key, DiscussionTools ID).
/// Our dated comments by key: each one's parent key and DiscussionTools ID.
type Ours = HashMap<Key, Vec<(Option<Key>, Option<String>)>>;

fn ours(threads: &[Thread]) -> Ours {
    let mut out: HashMap<Key, Vec<_>> = HashMap::new();
    for t in threads {
        let key = |k: usize| {
            let p = &t.posts[k];
            matches!(
                p.kind,
                PostKind::Comment | PostKind::Closing { .. } | PostKind::Collapse { .. }
            )
            .then(|| {
                p.signature
                    .as_ref()
                    .and_then(|s| s.when.map(|w| (s.user.clone(), w)))
            })
            .flatten()
        };
        for k in 0..t.posts.len() {
            if let Some(me) = key(k) {
                let parent = t.posts[k].parent.and_then(key);
                out.entry(me)
                    .or_default()
                    .push((parent, t.posts[k].dt.as_ref().map(|d| d.id.clone())));
            }
        }
    }
    out
}

/// The DiscussionTools keys of the comments in sections that fell back.
fn fallback_keys(
    threads: &[Thread],
    text: &str,
    markup: &Markup,
    site: &Site,
) -> std::collections::HashSet<Key> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = std::collections::HashSet::new();
    for t in threads.iter().filter(|t| t.fallback.is_some()) {
        for p in &t.posts {
            for i in p.lines.clone() {
                for sig in scatter_talk::signatures(lines[i], markup, site) {
                    if let Some(w) = sig.when {
                        out.insert((sig.user, w));
                    }
                }
            }
        }
    }
    out
}

#[test]
fn agrees_with_discussiontools() {
    let dir = samples();
    let manifest: Value =
        serde_json::from_slice(&read_sample(&dir, "manifest.json")).expect("json");
    let mut texts: BTreeMap<(String, u64), String> = BTreeMap::new();
    for dump in manifest["dumps"].as_object().expect("dumps").values() {
        for (title, id, _, _, text) in read_dump(&dir, dump["file"].as_str().expect("file")) {
            texts.insert((title, id), text);
        }
    }
    let (markup, site, opts) = (Markup::embedded(), Site::english(), Options::default());
    let (mut dt_total, mut both, mut parents_ok, mut parents_n, mut ids_ok, mut fallback_total) =
        (0, 0, 0, 0, 0, 0);
    for p in manifest["pages"].as_array().expect("pages") {
        let Some(file) = p["discussiontools"]["file"].as_str() else {
            continue;
        };
        let title = p["title"].as_str().expect("title").to_string();
        let text = &texts[&(title.clone(), p["revid"].as_u64().expect("revid"))];
        let conv = convert(text, markup, &site, &opts);
        let mine = ours(&conv.threads);
        let theirs = dt_comments(&dir, file);
        let in_fallback = fallback_keys(&conv.threads, text, markup, &site);
        let (mut b, mut po, mut pn, mut io, mut fb) = (0, 0, 0, 0, 0);
        for (key, parent, id) in &theirs {
            if in_fallback.contains(key) {
                fb += 1;
                continue;
            }
            let Some(found) = mine.get(key) else { continue };
            b += 1;
            if found.len() == 1 {
                pn += 1;
                po += usize::from(&found[0].0 == parent);
                io += usize::from(found[0].1.as_deref() == Some(id.as_str()));
            }
        }
        let n_ours: usize = mine.values().map(Vec::len).sum();
        let mut why: BTreeMap<&str, usize> = BTreeMap::new();
        for t in &conv.threads {
            if let Some(f) = t.fallback {
                *why.entry(f.reason()).or_insert(0) += 1;
                if std::env::var_os("TALK_SHOW").is_some_and(|v| v.to_string_lossy() == title) {
                    println!("  fallback {:?} in {:?}", f, t.subject);
                }
            }
        }
        let closings = conv
            .posts()
            .filter(|(_, p)| matches!(p.kind, PostKind::Closing { .. }))
            .count();
        let collapses = conv
            .posts()
            .filter(|(_, p)| matches!(p.kind, PostKind::Collapse { .. }))
            .count();
        if closings + collapses > 0 {
            println!("  closing posts {closings}, collapsing posts {collapses}");
        }
        if !why.is_empty() {
            println!("  fallbacks of {} threads: {why:?}", conv.threads.len());
        }
        println!(
            "{title}: ours {n_ours}, DiscussionTools {}, in fallback sections {fb}, both {b}; parents {po}/{pn}; IDs {io}/{pn}",
            theirs.len()
        );
        dt_total += theirs.len() - fb;
        fallback_total += fb;
        both += b;
        parents_ok += po;
        parents_n += pn;
        ids_ok += io;
    }
    println!(
        "total outside fallback sections: both {both}/{dt_total}; parents {parents_ok}/{parents_n}; IDs {ids_ok}/{parents_n}; {fallback_total} in fallback sections"
    );
    // The prototype, which had no fallbacks: 2,178 of 2,179 found, 97.5% of parents, 97.1% of
    // IDs. Sections that fall back (0090 §4) hold their comments whole, so they are counted apart;
    // since wrappers became closing and collapsing posts (0090 A1), none on the 2026-10-21 samples.
    assert!(
        fallback_total * 100 <= dt_total + fallback_total,
        "fallback sections: {fallback_total}"
    );
    assert!(
        both * 1000 >= dt_total * 999,
        "comments found: {both}/{dt_total}"
    );
    assert!(
        parents_ok * 1000 >= parents_n * 975,
        "parents: {parents_ok}/{parents_n}"
    );
    assert!(ids_ok * 100 >= parents_n * 97, "IDs: {ids_ok}/{parents_n}");
}

#[test]
fn attribution_from_history() {
    let revs = read_dump(&samples(), "history.xml.gz");
    let (markup, site) = (Markup::embedded(), Site::english());
    let mut blame = Blame::new(markup, &site);
    let mut by_source: BTreeMap<String, usize> = BTreeMap::new();
    let mut renamed = 0;
    for (k, r) in revs.iter().enumerate() {
        blame.push(
            Revision {
                id: r.1,
                actor: r.2.clone(),
                timestamp: stamp(&r.3),
            },
            &r.4,
        );
        if k % 250 != 249 && k + 1 != revs.len() {
            continue;
        }
        let mut conv = convert(&r.4, markup, &site, &Options::default());
        attribute(&mut conv, &r.4, Some(&blame), "Importer", markup, &site);
        for (_, p) in conv.posts() {
            let a = p.attribution.as_ref().expect("attributed");
            *by_source.entry(format!("{:?}", a.source)).or_insert(0) += 1;
            if a.signed_as.is_some() {
                assert_ne!(
                    a.author, "MediaWiki message delivery",
                    "a delivery bot is never a signed post's author"
                );
                if a.author == "SineBot" {
                    assert_eq!(
                        a.signed_as.as_deref(),
                        Some("SineBot"),
                        "SineBot is the author only of its own notices"
                    );
                }
            }
            if a.source == scatter_talk::AuthorSource::History {
                assert_eq!(
                    a.added_by.as_ref().map(|r| r.actor.as_str()),
                    Some(a.author.as_str())
                );
            }
            if a.signed_as.as_deref() == Some("Messedrocker")
                && a.added_by.as_ref().is_some_and(|r| r.actor == "Harej")
            {
                assert_eq!(a.author, "Harej", "a former name resolves to the account");
                renamed += 1;
            }
        }
    }
    println!(
        "{} revisions, attributed at every 250th: {by_source:?}; former names resolved: {renamed}",
        revs.len()
    );
    assert!(
        blame.is_alias("Harej", "Messedrocker"),
        "a rename is found in the history"
    );
    assert!(
        !blame.is_alias("MediaWiki message delivery", "Harej"),
        "a delivery bot takes no sender's name"
    );
    assert!(renamed > 0);
    let history = by_source.get("History").copied().unwrap_or(0);
    let signature = by_source.get("Signature").copied().unwrap_or(0);
    assert!(
        history > signature,
        "history agrees with most signatures: {by_source:?}"
    );
}
