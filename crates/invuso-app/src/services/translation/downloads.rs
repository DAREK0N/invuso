//! Running pack downloads with progress for the UI (SET-07).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dioxus::prelude::*;

use super::packs::{self, Pack, PackError};
use crate::platform;
use crate::state::DataRevision;

/// State of a download that has not finished successfully; a finished one
/// leaves no entry, the pack is then installed.
#[derive(Debug, Clone, PartialEq)]
pub enum PackDownload {
    Running { done: u64, total: u64 },
    Failed(String),
}

/// Downloads of this app run, by pack id. Provided above the router, so a
/// download goes on when its screen is left.
#[derive(Clone, Copy, PartialEq)]
pub struct PackDownloads {
    states: Signal<HashMap<String, PackDownload>>,
    cancels: Signal<HashMap<String, Arc<AtomicBool>>>,
}

impl PackDownloads {
    /// Must be called inside a component, like any `Signal::new`.
    pub fn new() -> Self {
        Self {
            states: Signal::new(HashMap::new()),
            cancels: Signal::new(HashMap::new()),
        }
    }

    /// The download of a pack; subscribes the caller to its changes.
    pub fn get(&self, pack: &Pack) -> Option<PackDownload> {
        self.states.read().get(&pack.id()).cloned()
    }

    /// Starts downloading `pack` unless that is already running. When done,
    /// `revision` is bumped so screens see the installed pack.
    pub fn start(&self, pack: &'static Pack, mut revision: DataRevision) {
        let (mut states, mut cancels) = (self.states, self.cancels);
        let id = pack.id();
        if matches!(states.peek().get(&id), Some(PackDownload::Running { .. })) {
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        cancels.write().insert(id.clone(), cancelled.clone());
        states.write().insert(
            id.clone(),
            PackDownload::Running {
                done: 0,
                total: pack.size(),
            },
        );
        dioxus::core::spawn_forever(async move {
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            let worker = tokio::task::spawn_blocking(move || {
                let data_dir = platform::data_dir().map_err(PackError::Storage)?;
                packs::download(
                    &data_dir,
                    pack,
                    &mut |done, total| {
                        // The receiver only goes away with the app.
                        let _ = sender.send((done, total));
                    },
                    &|| cancelled.load(Ordering::Relaxed),
                )
            });
            // Ends when the worker drops the sender.
            while let Some((done, total)) = receiver.recv().await {
                // Megabyte steps are enough for the bar and keep renders few.
                let step = |bytes: u64| bytes / 1_000_000;
                let shown = matches!(
                    states.peek().get(&id),
                    Some(PackDownload::Running { done: old, .. }) if step(*old) == step(done)
                );
                if !shown || done == total {
                    states
                        .write()
                        .insert(id.clone(), PackDownload::Running { done, total });
                }
            }
            cancels.write().remove(&id);
            match worker.await.map_err(|e| e.to_string()) {
                Ok(Ok(_)) => {
                    states.write().remove(&id);
                }
                Ok(Err(error)) => {
                    states
                        .write()
                        .insert(id, PackDownload::Failed(error.to_string()));
                }
                Err(error) => {
                    states.write().insert(id, PackDownload::Failed(error));
                }
            }
            revision.bump();
        });
    }

    /// Stops a running download; its partial files are removed.
    pub fn cancel(&self, pack: &Pack) {
        if let Some(flag) = self.cancels.peek().get(&pack.id()) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Forgets a failed download, e.g. before trying again.
    pub fn clear(&self, pack: &Pack) {
        let mut states = self.states;
        states.write().remove(&pack.id());
    }
}

impl Default for PackDownloads {
    fn default() -> Self {
        Self::new()
    }
}
