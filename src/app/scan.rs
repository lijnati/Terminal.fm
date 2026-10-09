//! Background library scanning.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};

use crate::library::Track;
use crate::library::scan::{ScanProgress, ScanResult, scan};

pub enum ScanMsg {
    Progress(ScanProgress),
    Done(ScanResult),
}

/// A scan running on a worker thread.
pub struct ScanJob {
    rx: Receiver<ScanMsg>,
    pub discovered: Option<usize>,
    pub done: usize,
    /// Show the full-screen progress view (first run) rather than a status line.
    pub foreground: bool,
}

impl ScanJob {
    pub fn start(roots: Vec<PathBuf>, previous: Vec<Track>, foreground: bool) -> Self {
        let (tx, rx) = sync_channel(32);
        std::thread::Builder::new()
            .name("terminalfm-scan".into())
            .spawn(move || {
                let result = scan(&roots, &previous, |p| {
                    // Progress is best-effort; never stall the scan on a slow UI.
                    let _ = tx.try_send(ScanMsg::Progress(p));
                });
                let _ = tx.send(ScanMsg::Done(result));
            })
            .expect("spawn scan thread");
        Self { rx, discovered: None, done: 0, foreground }
    }

    /// Applies pending progress; returns the result once the scan completes.
    pub fn poll(&mut self) -> Option<ScanResult> {
        loop {
            match self.rx.try_recv() {
                Ok(ScanMsg::Progress(ScanProgress::Discovered(n))) => self.discovered = Some(n),
                Ok(ScanMsg::Progress(ScanProgress::Read { done, total })) => {
                    self.done = done;
                    self.discovered = Some(total);
                }
                Ok(ScanMsg::Done(r)) => return Some(r),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => return Some(ScanResult::default()),
            }
        }
    }
}
