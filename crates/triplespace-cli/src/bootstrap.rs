//! The parallel side of a bootstrap load (0013 §9; 05 §3.7): one thread reads the dump's
//! lines, worker threads parse each into an entity, put it through the adapter and
//! prepare its record ([`Prepare::item`]: the hash check, the operation, its encoding and
//! the part hashes), and the caller's thread takes the prepared records back in dump
//! order for [`scatter_ingest::run_bootstrap`] to append. The ordinary sync does all of
//! this on one thread, between round trips.
//!
//! A JSON dump only: its lines are independent, so they can be parsed apart. Bounded
//! channels keep the lines in flight to a few blocks per worker, so memory does not grow
//! with the dump.

use std::collections::{BTreeMap, VecDeque};
use std::io::BufRead;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use scatter_adapter_wikibase::{KeyMappedAdapter, Outcome, parse_line};
use scatter_ingest::sync::SyncItem;
use scatter_ingest::{IngestError, Prepare, Prepared};

/// Lines per unit handed to a worker.
const LINES_PER_UNIT: usize = 64;

type Unit = (u64, Vec<(usize, String)>);
type Done = (u64, Vec<Result<Prepared, IngestError>>);

/// What the workers tally while the caller consumes: entities the reader, parser or
/// adapter could not read, and the newest upstream time seen (epoch seconds), where the
/// load ends for the follower (0092 §5).
#[derive(Debug, Default)]
pub struct Tally {
    /// Unreadable entities.
    pub unreadable: AtomicU64,
    /// The newest upstream time, 0 when none.
    pub latest: AtomicU64,
}

/// The prepared records of `dump`, in dump order, prepared on `workers` threads.
///
/// # Errors
///
/// When the dump cannot be opened.
pub fn prepared(
    dump: &Path,
    adapter: &Arc<KeyMappedAdapter>,
    prepare: Prepare,
    workers: usize,
    tally: &Arc<Tally>,
) -> Result<Ordered, IngestError> {
    let reader = scatter_adapter_wikibase::open(dump).map_err(|e| e.to_string())?;
    let workers = workers.max(1);
    let (unit_tx, unit_rx) = sync_channel::<Unit>(workers * 2);
    let (done_tx, done_rx) = sync_channel::<Done>(workers * 4);
    std::thread::spawn({
        let tally = Arc::clone(tally);
        move || read_units(reader, &unit_tx, &tally)
    });
    let unit_rx = Arc::new(Mutex::new(unit_rx));
    let prepare = Arc::new(prepare);
    for _ in 0..workers {
        let unit_rx = Arc::clone(&unit_rx);
        let done_tx = done_tx.clone();
        let adapter = Arc::clone(adapter);
        let prepare = Arc::clone(&prepare);
        let tally = Arc::clone(tally);
        std::thread::spawn(move || {
            loop {
                let unit = unit_rx.lock().expect("not poisoned").recv();
                let Ok((seq, lines)) = unit else { break };
                let out = lines
                    .into_iter()
                    .flat_map(|(line, text)| prepare_line(line, &text, &adapter, &prepare, &tally))
                    .collect();
                if done_tx.send((seq, out)).is_err() {
                    break;
                }
            }
        });
    }
    drop(done_tx);
    Ok(Ordered {
        rx: done_rx,
        pending: BTreeMap::new(),
        next: 0,
        current: VecDeque::new(),
    })
}

/// The reader thread: the dump's lines, `LINES_PER_UNIT` at a time, numbered in order.
fn read_units(mut reader: Box<dyn BufRead + Send>, tx: &SyncSender<Unit>, tally: &Tally) {
    let mut seq = 0u64;
    let mut line = 0usize;
    let mut unit = Vec::with_capacity(LINES_PER_UNIT);
    loop {
        let mut text = String::new();
        match reader.read_line(&mut text) {
            Ok(0) => break,
            Ok(_) => {
                line += 1;
                unit.push((line, text));
            }
            Err(e) => {
                tally.unreadable.fetch_add(1, Ordering::Relaxed);
                eprintln!("dump error: line {}: {e}", line + 1);
                break;
            }
        }
        if unit.len() == LINES_PER_UNIT {
            if tx.send((seq, std::mem::take(&mut unit))).is_err() {
                return;
            }
            seq += 1;
        }
    }
    if !unit.is_empty() {
        let _ = tx.send((seq, unit));
    }
}

/// One line's prepared records: none for a bracket or an unreadable entity, two for a
/// reject that still has a state, as [`crate::sync::SyncItems`] hands them over.
fn prepare_line(
    line: usize,
    text: &str,
    adapter: &KeyMappedAdapter,
    prepare: &Prepare,
    tally: &Tally,
) -> Vec<Result<Prepared, IngestError>> {
    let parsed = match parse_line(line, text) {
        None => return Vec::new(),
        Some(Ok(p)) => p,
        Some(Err(e)) => {
            tally.unreadable.fetch_add(1, Ordering::Relaxed);
            eprintln!("dump error: {e}");
            return Vec::new();
        }
    };
    let items = match adapter.sync_item(parsed) {
        Ok(Outcome::State {
            entity,
            upstream,
            upstream_id,
            upstream_aliases,
        }) => vec![SyncItem::State {
            entity,
            upstream,
            upstream_id,
            upstream_aliases,
        }],
        Ok(Outcome::Rejected {
            entity,
            upstream,
            upstream_id,
            reason,
        }) => vec![
            SyncItem::Rejected {
                upstream_id,
                reason,
            },
            SyncItem::State {
                entity,
                upstream,
                upstream_id: None,
                upstream_aliases: Vec::new(),
            },
        ],
        Err(e) => {
            tally.unreadable.fetch_add(1, Ordering::Relaxed);
            eprintln!("adapter error: {e}");
            return Vec::new();
        }
    };
    items
        .into_iter()
        .map(|item| {
            if let SyncItem::State { upstream, .. } = &item
                && let Some(t) = upstream
                    .time
                    .as_deref()
                    .and_then(scatter_adapter_mediawiki::recentchanges::parse_iso)
            {
                tally.latest.fetch_max(t, Ordering::Relaxed);
            }
            prepare.item(item)
        })
        .collect()
}

/// The workers' output, put back in dump order.
pub struct Ordered {
    rx: Receiver<Done>,
    pending: BTreeMap<u64, Vec<Result<Prepared, IngestError>>>,
    next: u64,
    current: VecDeque<Result<Prepared, IngestError>>,
}

impl Iterator for Ordered {
    type Item = Result<Prepared, IngestError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.current.pop_front() {
                return Some(item);
            }
            if let Some(unit) = self.pending.remove(&self.next) {
                self.next += 1;
                self.current.extend(unit);
                continue;
            }
            // Every worker has finished and nothing is pending: the dump is done.
            let (seq, unit) = self.rx.recv().ok()?;
            self.pending.insert(seq, unit);
        }
    }
}
