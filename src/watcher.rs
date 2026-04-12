use std::sync::mpsc;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc as tokio_mpsc;

use crate::pipeline;
use crate::server::AppState;

pub async fn watch(
    state: AppState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (async_tx, mut async_rx) = tokio_mpsc::unbounded_channel::<()>();

    // Spawn a dedicated OS thread to own the notify watcher and forward its
    // sync events onto a tokio channel. The watcher has to stay alive for the
    // lifetime of the thread or its callbacks stop firing.
    let watch_dir = state.notes_dir.clone();
    std::thread::spawn(move || {
        let (sync_tx, sync_rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher: RecommendedWatcher = match notify::recommended_watcher(sync_tx) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[watcher] init failed: {}", e);
                return;
            }
        };
        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::Recursive) {
            eprintln!("[watcher] failed to watch {}: {}", watch_dir.display(), e);
            return;
        }
        println!("[watcher] watching {}", watch_dir.display());

        while let Ok(event) = sync_rx.recv() {
            if let Ok(evt) = event {
                if !is_interesting(&evt) {
                    continue;
                }
            }
            if async_tx.send(()).is_err() {
                break;
            }
        }
    });

    // Debounce loop. On the first event, drain any follow-ups within a 300ms
    // quiet window, then re-ingest once. Editors often fire several inotify
    // events per save (write / chmod / close) which we coalesce here.
    while async_rx.recv().await.is_some() {
        loop {
            tokio::select! {
                more = async_rx.recv() => {
                    if more.is_none() {
                        return Ok(());
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(300)) => break,
            }
        }

        let db = state.db.clone();
        let dir = state.notes_dir.clone();
        match tokio::task::spawn_blocking(move || pipeline::load(db.as_ref(), &dir)).await {
            Ok(stats) => {
                println!(
                    "[watcher] re-ingested: {} notes, {} links",
                    stats.notes, stats.links
                );
                // Tell all connected clients that a newer graph is available.
                // Clients decide whether to refetch — we don't force it.
                let _ = state.reload_tx.send(());
            }
            Err(e) => eprintln!("[watcher] reload task failed: {}", e),
        }
    }

    Ok(())
}

fn is_interesting(event: &notify::Event) -> bool {
    use notify::EventKind;
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event
        .paths
        .iter()
        .any(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
}
