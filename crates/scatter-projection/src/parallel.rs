//! Work spread over the machine's threads: what a block of records needs done record by
//! record, where each record's share is a pure function of it (0013 §9, A74, A76).

/// `items` mapped by `f` on as many threads as the machine has, in order. Each thread takes
/// the next item when it finishes one, rather than a fixed share: a block's records differ
/// in size by three orders of magnitude (the first Wikidata items run to megabytes), and a
/// share holding the largest would keep the others waiting.
///
/// # Panics
///
/// When `f` panics, with its panic.
pub fn par_map<T: Send, U: Send>(items: Vec<T>, f: impl Fn(T) -> U + Sync) -> Vec<U> {
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(items.len());
    if threads < 2 {
        return items.into_iter().map(f).collect();
    }
    let slots: Vec<std::sync::Mutex<Option<T>>> = items
        .into_iter()
        .map(|t| std::sync::Mutex::new(Some(t)))
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<Option<U>> = std::iter::repeat_with(|| None).take(slots.len()).collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(slot) = slots.get(i) else { break };
                        let item = slot
                            .lock()
                            .expect("not poisoned")
                            .take()
                            .expect("each item is taken once");
                        done.push((i, f(item)));
                    }
                    done
                })
            })
            .collect();
        for h in handles {
            for (i, u) in h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)) {
                out[i] = Some(u);
            }
        }
    });
    out.into_iter()
        .map(|u| u.expect("every item is mapped"))
        .collect()
}
