//! Planned records replay as valid threads: on short pages written for each rule, and on every
//! page of `scatter-talk`'s fixtures (0090 §13).

use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use scatter_talk::{Markup, Options, Site, attribute, convert};
use scatter_talk_import::{CLOSED, Origin, PlannedThread, Source, plain_text, plan, section_hash};
use scatter_threads::{Check, Limits, Minted, Record, Target, Thread, ThreadOp, Writer};

fn import() -> Check<'static> {
    Check {
        writer: Writer::Import,
        limits: Limits::default(),
        ..Check::replay()
    }
}

/// Appends a planned thread as a job would, numbering records from `next`, and folds it.
fn replay(t: &PlannedThread, next: &mut u64) -> Thread {
    let home = Target::Page(42);
    let mut revids = Vec::new();
    let mut thread: Option<Thread> = None;
    for (k, r) in t.records.iter().enumerate() {
        let op = t.op(k, &home, &revids);
        assert_eq!(
            ThreadOp::from_json(op.to_json()).as_ref(),
            Ok(&op),
            "content part round-trips"
        );
        let rec = Record {
            revid: *next,
            actor: r.author.clone().unwrap_or_else(|| "importer".into()),
            time: 0,
            op,
            minted: Minted::default(),
            has_text: true,
        };
        match &mut thread {
            None => {
                thread = Some(
                    Thread::create(&rec, &import())
                        .unwrap_or_else(|e| panic!("{}: create: {e}", t.subject)),
                );
            }
            Some(th) => th
                .apply(&rec, &import())
                .unwrap_or_else(|e| panic!("{}: record {k}: {e}", t.subject)),
        }
        revids.push(*next);
        *next += 1;
    }
    thread.expect("a thread has a create")
}

fn run(text: &str) -> Vec<PlannedThread> {
    let conv = convert(
        text,
        Markup::embedded(),
        &Site::english(),
        &Options::default(),
    );
    let source = Source {
        repository: Some("enwiki".into()),
        page: "Talk:X".into(),
        revision: Some(7),
        archive: None,
    };
    plan(&conv, "Talk:X", &source)
}

#[test]
fn a_page_plans_a_header_and_threads() {
    let page = "{{Talk header}}\n== [[Foo|The foo]] {{anchor|x}} question ==\nWhy? [[User:A|A]] 12:00, 5 June 2025 (UTC)\n:Because. [[User:B|B]] 12:05, 5 June 2025 (UTC)";
    let plans = run(page);
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].origin, Origin::Header);
    assert_eq!(plans[0].subject, "Talk:X: talk page header");
    assert!(plans[0].records[0].pinned);
    assert_eq!(plans[1].subject, "The foo question");
    let t = &plans[1];
    assert_eq!(t.records[1].reply_to, Some(0));
    assert_eq!(t.records[0].author.as_deref(), Some("A"));
    let from = &t.records[0].imported_from;
    assert_eq!(from.page.as_deref(), Some("Talk:X"));
    assert_eq!(from.revision, Some(7));
    assert!(from.section_hash.as_ref().is_some_and(|h| h.len() == 32));
    assert_eq!(from.name.as_deref(), Some("c-A-20250605120000"));
    assert_eq!(from.timestamp.as_deref(), Some("2025-06-05T12:00:00Z"));
    let mut next = 1;
    let header = replay(&plans[0], &mut next);
    assert_eq!(header.status(), "open");
    assert!(header.home.pinned.is_some());
    let th = replay(t, &mut next);
    assert_eq!(th.status(), "archived");
    assert_eq!(th.posts()[1].parent, Some(th.created));
}

#[test]
fn a_closed_section_ends_closed_and_a_hatted_range_collapses() {
    let page = "== A ==\n{{atop|result=No consensus. [[User:C|C]] 13:00, 5 June 2025 (UTC)}}\nQ [[User:A|A]] 12:00, 5 June 2025 (UTC)\n:R [[User:B|B]] 12:05, 5 June 2025 (UTC)\n{{abot}}\n== B ==\nQ [[User:A|A]] 12:00, 6 June 2025 (UTC)\n{{hat|Tangent}}\n:S [[User:B|B]] 12:05, 6 June 2025 (UTC)\n{{hab}}";
    let plans = run(page);
    let mut next = 1;
    let closed = replay(&plans[0], &mut next);
    assert_eq!(closed.status(), CLOSED);
    let closing = closed.posts().last().unwrap();
    assert_eq!(closing.parent, None);
    assert_eq!(closing.actor, "C");
    let hatted = replay(&plans[1], &mut next);
    assert_eq!(hatted.status(), "archived");
    assert!(hatted.is_collapsed(hatted.posts()[1].revid));
    assert!(!hatted.is_collapsed(hatted.created));
}

#[test]
fn a_section_opening_with_a_subheading_keeps_it_as_its_first_post() {
    let page = "== Vote ==\n=== Support ===\n# Yes [[User:A|A]] 12:00, 5 June 2025 (UTC)\n=== Oppose ===\n# No [[User:B|B]] 12:05, 5 June 2025 (UTC)";
    let plans = run(page);
    let t = &plans[0];
    assert_eq!(t.records[0].text, "=== Support ===");
    assert_eq!(t.records[0].heading, None);
    assert_eq!(t.records[1].reply_to, Some(0));
    assert_eq!(t.records[2].heading, Some(3));
    let mut next = 1;
    let th = replay(t, &mut next);
    assert_eq!(th.posts()[3].parent, Some(th.posts()[2].revid));
}

#[test]
fn subjects_lose_markup_and_forbidden_characters() {
    assert_eq!(
        plain_text("[[Q42|Douglas]] ''Adams'' {{tl|x}} <small>[sic]</small>"),
        "Douglas Adams  sic"
    );
    let plans = run("== {{subst:void}} ==\nQ [[User:A|A]] 12:00, 5 June 2025 (UTC)");
    assert_eq!(plans[0].subject, "Untitled section");
    assert_eq!(
        section_hash("h-A-20250605120000"),
        section_hash("h-A-20250605120000")
    );
    assert_ne!(section_hash("a"), section_hash("b"));
}

type Rev = (String, u64, String, String, String);

fn read_dump(path: &Path) -> Vec<Rev> {
    let mut raw = Vec::new();
    bzip2::read::BzDecoder::new(std::fs::File::open(path).expect("fixture"))
        .read_to_end(&mut raw)
        .expect("bzip2");
    let mut reader = Reader::from_reader(raw.as_slice());
    let (mut out, mut stack, mut buf, mut leaf, mut title) = (
        Vec::new(),
        Vec::<String>::new(),
        Vec::new(),
        String::new(),
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

#[test]
fn every_fixture_page_plans_threads_that_replay() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/samples/enwiki-talk");
    let mut pages = read_dump(&dir.join("current.xml.bz2"));
    if let Some(last) = read_dump(&dir.join("history.xml.bz2")).pop() {
        pages.push(last);
    }
    let (markup, site) = (Markup::embedded(), Site::english());
    let (mut threads, mut records, mut closed, mut collapses) = (0, 0, 0, 0);
    let mut next = 1;
    for (title, revid, _, _, text) in &pages {
        let mut conv = convert(text, markup, &site, &Options::default());
        attribute(&mut conv, text, None, "Importer", markup, &site);
        let source = Source {
            repository: Some("enwiki".into()),
            page: title.clone(),
            revision: Some(*revid),
            archive: None,
        };
        let posts: usize = conv.threads.iter().map(|t| t.posts.len()).sum();
        let plans = plan(&conv, title, &source);
        let planned: usize = plans
            .iter()
            .filter(|p| p.origin != Origin::Header)
            .map(|p| p.records.len())
            .sum();
        assert_eq!(planned, posts, "{title}: one record per post");
        for p in &plans {
            let th = replay(p, &mut next);
            threads += 1;
            records += th.posts().len();
            closed += usize::from(th.status() == CLOSED);
            collapses += th.collapses().count();
            assert!(!p.subject.is_empty());
        }
    }
    println!(
        "{} pages: {threads} threads, {records} records, {closed} closed, {collapses} collapsed ranges",
        pages.len()
    );
    assert!(threads > 150 && closed >= 11 && collapses >= 6);
}
