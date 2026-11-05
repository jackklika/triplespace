//! The pipeline over `MemoryStore` and `MemoryBackend`, with two toy projections: a
//! step-1 counter of records per payload type, and a step-4 index by key whose records
//! fan out to the keys their content names.

use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::header::Header;
use scatter_log::memory::MemoryStore;
use scatter_log::record::Record;
use scatter_log::store::{Draft, LogStore};
use scatter_log::tree::Segments;
use scatter_projection::memory::MemoryCx;
use scatter_projection::{
    Applied, Backend, BoxFuture, Budget, MemoryBackend, Pipeline, Projection, ProjectionError,
    Step, Target, Worker,
};

const PARTITION: u64 = 7;

fn draft(n: u64, key: &str, refs: &[&str]) -> Draft {
    Draft {
        appended_at: 1_790_000_000_000_000 + n,
        payload_type: if key.starts_with('Q') {
            "scatter:v0/changeset".into()
        } else {
            "scatter:v0/other".into()
        },
        key: Some(key.into()),
        revid: Some(n + 1),
        logid: None,
        page_id: None,
        body: Body::core(
            &Value::map(vec![
                (Value::text("n"), Value::Int(i128::from(n))),
                (
                    Value::text("refs"),
                    Value::Array(refs.iter().map(|r| Value::text(r)).collect()),
                ),
            ]),
            &Value::Null,
            &Value::map(vec![(Value::text("actor"), Value::text("local:1"))]),
        )
        .unwrap(),
    }
}

/// Step 1: counts records per payload type.
struct Counter;

impl Projection<MemoryBackend> for Counter {
    fn name(&self) -> &'static str {
        "counter"
    }
    fn step(&self) -> Step {
        Step::Registry
    }
    fn apply<'a>(
        &'a self,
        cx: &'a mut MemoryCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let key = ("counter".to_string(), record.header().payload_type.clone());
            let n: u64 = cx.state.rows.get(&key).map_or(0, |v| v.parse().unwrap());
            cx.state.rows.insert(key, (n + 1).to_string());
            Ok(Applied::rows(1))
        })
    }
    fn reset<'a>(
        &'a self,
        cx: &'a mut MemoryCx,
        _partition: u64,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            cx.clear("counter", "");
            Ok(())
        })
    }
}

/// Step 4: the latest `n` per key, for changesets only; fans out to the keys the record
/// refers to, writing a `referrers` row for each; fails on a key named `BOOM`.
struct Index;

impl Projection<MemoryBackend> for Index {
    fn name(&self) -> &'static str {
        "index"
    }
    fn step(&self) -> Step {
        Step::Resolution
    }
    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == "scatter:v0/changeset"
    }
    fn apply<'a>(
        &'a self,
        cx: &'a mut MemoryCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let key = record.header().key.clone().unwrap();
            if key.contains("BOOM") {
                return Err("boom".into());
            }
            let content = record.body().content().value().unwrap().unwrap();
            let n = content.get("n").and_then(Value::as_u64).unwrap();
            cx.put("index", &key, &n.to_string());
            let fanout = match content.get("refs") {
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(Value::as_text)
                    .map(|k| Target {
                        tenant: String::new(),
                        key: k.to_string(),
                    })
                    .collect(),
                _ => Vec::new(),
            };
            Ok(Applied { rows: 1, fanout })
        })
    }
    fn fanout<'a>(
        &'a self,
        cx: &'a mut MemoryCx,
        _tenant: &'a str,
        key: &'a str,
    ) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async move {
            let k = ("referrers".to_string(), key.to_string());
            let n: u64 = cx.state.rows.get(&k).map_or(0, |v| v.parse().unwrap());
            cx.state.rows.insert(k, (n + 1).to_string());
            Ok(1)
        })
    }
    fn reset<'a>(
        &'a self,
        cx: &'a mut MemoryCx,
        _partition: u64,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            cx.clear("index", "");
            cx.clear("referrers", "");
            Ok(())
        })
    }
}

fn pipeline() -> Pipeline<MemoryBackend> {
    // Added out of order; the pipeline sorts by step.
    Pipeline::new().with(Index).unwrap().with(Counter).unwrap()
}

async fn store_with(records: &[(&str, &[&str])]) -> MemoryStore {
    let mut store = MemoryStore::new();
    store
        .create_partition(PARTITION, Segments::new(2).unwrap())
        .await
        .unwrap();
    for (n, (key, refs)) in records.iter().enumerate() {
        store
            .append(PARTITION, draft(n as u64, key, refs))
            .await
            .unwrap();
    }
    store
}

fn row(backend: &MemoryBackend, table: &str, key: &str) -> Option<String> {
    backend
        .state()
        .rows
        .get(&(table.into(), key.into()))
        .cloned()
}

#[test]
fn ordering_and_names() {
    let p = pipeline();
    assert_eq!(
        p.projections().iter().map(|p| p.name()).collect::<Vec<_>>(),
        vec!["counter", "index"]
    );
    assert_eq!(p.steps(), vec![Step::Registry, Step::Resolution]);
    assert!(matches!(
        pipeline().with(Counter),
        Err(ProjectionError::DuplicateName(n)) if n == "counter"
    ));
    assert!(Step::Activity.is_synchronous());
    assert!(!Step::Addressing.is_synchronous());
}

#[test]
fn catch_up_in_batches_then_lag_is_zero() {
    pollster::block_on(async {
        let store = store_with(&[
            ("Q1", &["Q2"]),
            ("Q2", &[]),
            ("P1", &[]),
            ("Q3", &["Q1", "Q2"]),
            ("Q1", &[]),
        ])
        .await;
        let backend = MemoryBackend::new();
        let p = pipeline();
        let lags = p.lags(&backend, &store, PARTITION).await.unwrap();
        assert!(
            lags.iter()
                .all(|l| l.applied == 0 && l.head == 5 && l.behind() == 5)
        );

        let progress = p.catch_up(&backend, &store, PARTITION, 2).await.unwrap();
        assert_eq!(progress.records, 5);
        assert_eq!(progress.head, 5);
        // Counter saw every record; Index only the four changesets.
        assert_eq!(
            row(&backend, "counter", "scatter:v0/changeset").as_deref(),
            Some("4")
        );
        assert_eq!(
            row(&backend, "counter", "scatter:v0/other").as_deref(),
            Some("1")
        );
        assert_eq!(progress.rows, 5 + 4);
        assert_eq!(
            row(&backend, "index", "Q1").as_deref(),
            Some("4"),
            "the latest wins"
        );
        assert_eq!(row(&backend, "index", "Q3").as_deref(), Some("3"));
        assert!(row(&backend, "index", "P1").is_none());
        // Catching up queues fan-out rather than applying it.
        assert_eq!(progress.queued, 3);
        assert_eq!(backend.state().work.items.len(), 3);
        assert!(row(&backend, "referrers", "Q2").is_none());
        let lags = p.lags(&backend, &store, PARTITION).await.unwrap();
        assert!(
            lags.iter().all(|l| l.applied == 5 && l.behind() == 0),
            "{lags:?}"
        );
        // Nothing more to do.
        let again = p.catch_up(&backend, &store, PARTITION, 2).await.unwrap();
        assert_eq!(again.records, 0);

        // The worker applies the queued fan-out: three items, two distinct keys, each
        // key re-projected once (the fixture counts calls; a real fan-out recomputes).
        let worker = Worker::new().with_all(p.consumers()).unwrap();
        assert_eq!(worker.run_once(&backend).await.unwrap().applied, 3);
        assert_eq!(row(&backend, "referrers", "Q2").as_deref(), Some("1"));
        assert_eq!(row(&backend, "referrers", "Q1").as_deref(), Some("1"));
        assert!(backend.state().work.items.is_empty());
    });
}

#[test]
fn a_failing_record_rolls_its_batch_back() {
    pollster::block_on(async {
        let store = store_with(&[("Q1", &[]), ("Q2", &[]), ("QBOOM", &[]), ("Q4", &[])]).await;
        let backend = MemoryBackend::new();
        let p = pipeline();
        let err = p
            .catch_up(&backend, &store, PARTITION, 2)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ProjectionError::Failed { ref projection, partition: PARTITION, offset: 2, .. } if projection == "index"
        ));
        // The first batch (offsets 0–1) committed; the second did not.
        let lags = p.lags(&backend, &store, PARTITION).await.unwrap();
        assert!(lags.iter().all(|l| l.applied == 2), "{lags:?}");
        assert_eq!(row(&backend, "index", "Q2").as_deref(), Some("1"));
        assert!(row(&backend, "index", "Q4").is_none());
        assert_eq!(
            row(&backend, "counter", "scatter:v0/changeset").as_deref(),
            Some("2")
        );
    });
}

#[test]
fn compacted_offsets_are_skipped() {
    pollster::block_on(async {
        let mut store = store_with(&[("Q1", &[]), ("Q2", &[]), ("Q3", &[])]).await;
        store.compact(PARTITION, &[1]).await.unwrap();
        let backend = MemoryBackend::new();
        let p = pipeline();
        let progress = p.catch_up(&backend, &store, PARTITION, 10).await.unwrap();
        assert_eq!(progress.records, 3);
        assert!(row(&backend, "index", "Q2").is_none());
        assert_eq!(
            row(&backend, "counter", "scatter:v0/changeset").as_deref(),
            Some("2")
        );
        assert!(
            p.lags(&backend, &store, PARTITION)
                .await
                .unwrap()
                .iter()
                .all(|l| l.applied == 3)
        );
    });
}

#[test]
fn inline_application_within_a_budget() {
    pollster::block_on(async {
        let mut store = store_with(&[]).await;
        let backend = MemoryBackend::new();
        let p = pipeline();
        // Append, then apply inline in the same unit of work, as the write path does.
        let mut cx = backend.begin().await.unwrap();
        store
            .append(PARTITION, draft(0, "Q1", &["Q2", "Q3", "Q4"]))
            .await
            .unwrap();
        let record = store
            .read(PARTITION, 0)
            .await
            .unwrap()
            .record()
            .unwrap()
            .clone();
        let inline = p
            .apply_inline(
                &backend,
                &mut cx,
                &record,
                Budget {
                    rows: 2,
                    time: std::time::Duration::MAX,
                },
            )
            .await
            .unwrap();
        backend.commit(cx).await.unwrap();
        assert_eq!(inline.rows, 2, "counter + index");
        assert_eq!(inline.fanned_out, 2, "two referrers within the budget");
        assert_eq!(inline.queued, 1, "the third is queued");
        assert_eq!(row(&backend, "referrers", "Q2").as_deref(), Some("1"));
        assert_eq!(row(&backend, "referrers", "Q3").as_deref(), Some("1"));
        assert!(row(&backend, "referrers", "Q4").is_none());
        assert_eq!(backend.state().work.items[0].work.key, "Q4");
        // Positions advanced for both synchronous projections; a catch-up has nothing left.
        let lags = p.lags(&backend, &store, PARTITION).await.unwrap();
        assert!(lags.iter().all(|l| l.applied == 1 && l.behind() == 0));
        assert_eq!(
            p.catch_up(&backend, &store, PARTITION, 10)
                .await
                .unwrap()
                .records,
            0
        );

        // Budget::NONE queues everything; UNLIMITED applies everything.
        let mut cx = backend.begin().await.unwrap();
        store
            .append(PARTITION, draft(1, "Q5", &["Q6"]))
            .await
            .unwrap();
        let record = store
            .read(PARTITION, 1)
            .await
            .unwrap()
            .record()
            .unwrap()
            .clone();
        let inline = p
            .apply_inline(&backend, &mut cx, &record, Budget::NONE)
            .await
            .unwrap();
        assert_eq!((inline.fanned_out, inline.queued), (0, 1));
        store
            .append(PARTITION, draft(2, "Q7", &["Q8", "Q9"]))
            .await
            .unwrap();
        let record = store
            .read(PARTITION, 2)
            .await
            .unwrap()
            .record()
            .unwrap()
            .clone();
        let inline = p
            .apply_inline(&backend, &mut cx, &record, Budget::UNLIMITED)
            .await
            .unwrap();
        assert_eq!((inline.fanned_out, inline.queued), (2, 0));
        backend.commit(cx).await.unwrap();
        // A failing projection surfaces as Failed, with the record's position.
        let mut cx = backend.begin().await.unwrap();
        store
            .append(PARTITION, draft(3, "QBOOM", &[]))
            .await
            .unwrap();
        let record = store
            .read(PARTITION, 3)
            .await
            .unwrap()
            .record()
            .unwrap()
            .clone();
        assert!(matches!(
            p.apply_inline(&backend, &mut cx, &record, Budget::default())
                .await,
            Err(ProjectionError::Failed { offset: 3, .. })
        ));
        backend.rollback(cx).await.unwrap();
    });
}

#[test]
fn rebuild_replays_from_zero() {
    pollster::block_on(async {
        let store = store_with(&[("Q1", &["Q2"]), ("Q2", &[]), ("Q1", &[])]).await;
        let backend = MemoryBackend::new();
        let p = pipeline();
        p.catch_up(&backend, &store, PARTITION, 10).await.unwrap();
        // Corrupt the derived state, then rebuild.
        backend
            .state()
            .rows
            .insert(("index".into(), "Q1".into()), "wrong".into());
        backend
            .state()
            .rows
            .insert(("index".into(), "stray".into()), "x".into());
        let progress = p.rebuild(&backend, &store, &[PARTITION], 2).await.unwrap();
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].records, 3);
        assert_eq!(row(&backend, "index", "Q1").as_deref(), Some("2"));
        assert!(row(&backend, "index", "stray").is_none());
        assert_eq!(
            row(&backend, "counter", "scatter:v0/changeset").as_deref(),
            Some("3")
        );
    });
}

#[test]
fn backend_futures_are_send() {
    fn assert_send<T: Send>(_: &T) {}
    let backend = MemoryBackend::new();
    let store = MemoryStore::new();
    let p = pipeline();
    assert_send(&p.catch_up(&backend, &store, PARTITION, 1));
    assert_send(&p.lags(&backend, &store, PARTITION));
    let worker = Worker::new().with_all(p.consumers()).unwrap();
    assert_send(&worker.run_once(&backend));
}

/// 03 §6.1, replay order: the partitions of one group replay interleaved by append time,
/// so the record written last is applied last whichever partition holds it; replayed one
/// partition after another, the later partition would win.
#[test]
fn a_group_replays_in_append_order_across_partitions() {
    pollster::block_on(async {
        const OTHER: u64 = 8;
        let mut store = MemoryStore::new();
        for part in [PARTITION, OTHER] {
            store
                .create_partition(part, Segments::new(2).unwrap())
                .await
                .unwrap();
        }
        // Q1 is written first in OTHER (n = 0), then in PARTITION (n = 1).
        store.append(OTHER, draft(0, "Q1", &[])).await.unwrap();
        store.append(PARTITION, draft(1, "Q1", &[])).await.unwrap();
        store.append(OTHER, draft(2, "Q2", &[])).await.unwrap();
        let backend = MemoryBackend::new();
        let p = pipeline();

        p.rebuild(&backend, &store, &[PARTITION, OTHER], 10)
            .await
            .unwrap();
        assert_eq!(row(&backend, "index", "Q1").as_deref(), Some("0"));

        let progress = p
            .rebuild_grouped(&backend, &store, &[vec![PARTITION, OTHER]], 1)
            .await
            .unwrap();
        assert_eq!(row(&backend, "index", "Q1").as_deref(), Some("1"));
        assert_eq!(row(&backend, "index", "Q2").as_deref(), Some("2"));
        let records: Vec<(u64, u64)> = progress.iter().map(|(id, p)| (*id, p.records)).collect();
        assert_eq!(records, vec![(PARTITION, 1), (OTHER, 2)]);
        // Every projection's position is at each partition's head.
        assert!(
            p.lags(&backend, &store, OTHER)
                .await
                .unwrap()
                .iter()
                .all(|l| l.applied == l.head)
        );
    });
}
