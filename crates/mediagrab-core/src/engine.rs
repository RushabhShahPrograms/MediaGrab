//! The orchestrator: resolve links -> build a queue -> run downloads in parallel.
//!
//! Concurrency model (the part worth understanding):
//!   * One tokio task per item, spawned up front.
//!   * A `Semaphore` with `parallel` permits gates how many actually run.
//!     A task waits for a permit (state: Queued), downloads, then drops the permit.
//!   * All tasks report through ONE unbounded channel of `Event`s. The UI owns the
//!     receiving end, so the UI never blocks the downloads and vice versa.
//!   * Cancel is a `watch<bool>`: every task selects on it, and kills its yt-dlp child.

use crate::download::{cancelled, download_item};
use crate::resolve::resolve;
use crate::types::{Event, Item, ItemState, Options};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch, Semaphore};
use tokio::task::JoinSet;

pub struct Engine;

/// Keep this alive for as long as the batch runs; call `cancel()` to stop it.
#[derive(Clone)]
pub struct CancelHandle(Arc<watch::Sender<bool>>);

impl CancelHandle {
    pub fn cancel(&self) {
        let _ = self.0.send(true);
    }
}

impl Engine {
    /// Start a batch on the given tokio runtime. Returns immediately.
    pub fn start(
        rt: &Handle,
        inputs: Vec<String>,
        opts: Options,
    ) -> (mpsc::UnboundedReceiver<Event>, CancelHandle) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (ctx, crx) = watch::channel(false);
        rt.spawn(run(inputs, opts, tx, crx));
        (rx, CancelHandle(Arc::new(ctx)))
    }
}

async fn run(
    inputs: Vec<String>,
    opts: Options,
    tx: mpsc::UnboundedSender<Event>,
    mut cancel: watch::Receiver<bool>,
) {
    // ---- 1. resolve every pasted link into items ----------------------
    let mut items: Vec<Item> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for url in &inputs {
        let _ = tx.send(Event::Status(format!("Resolving {url} ...")));
        let first_id = items.len();
        let res = tokio::select! {
            r = resolve(&opts.ytdlp, &opts.extra_args, url, first_id) => r,
            _ = cancelled(&mut cancel) => {
                let _ = tx.send(Event::Finished);
                return;
            }
        };
        match res {
            Ok(found) => {
                for it in found {
                    if seen.insert(it.url.clone()) {
                        items.push(Item { id: items.len(), ..it });
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(Event::Error(format!("{url}: {e}")));
            }
        }
    }

    if items.is_empty() {
        let _ = tx.send(Event::Finished);
        return;
    }
    let _ = tx.send(Event::Status(format!("Found {} item(s)", items.len())));
    let _ = tx.send(Event::ItemsFound(items.clone()));

    // ---- 2. one task per item, gated by a semaphore --------------------
    let sem = Arc::new(Semaphore::new(opts.parallel.max(1)));
    let opts = Arc::new(opts);
    let mut set = JoinSet::new();
    for item in items {
        let (sem, opts, tx, mut cancel) = (sem.clone(), opts.clone(), tx.clone(), cancel.clone());
        set.spawn(async move {
            let permit = tokio::select! {
                p = sem.acquire_owned() => p.ok(),
                _ = cancelled(&mut cancel) => None,
            };
            let Some(_permit) = permit else {
                let _ = tx.send(Event::State { id: item.id, state: ItemState::Cancelled });
                return;
            };
            if *cancel.borrow() {
                let _ = tx.send(Event::State { id: item.id, state: ItemState::Cancelled });
                return;
            }
            download_item(&opts, &item, &tx, &mut cancel).await;
            // `_permit` drops here -> the next queued item may start
        });
    }
    while set.join_next().await.is_some() {}
    let _ = tx.send(Event::Finished);
}
