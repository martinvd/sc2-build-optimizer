//! Search status reporting, either as a one-line status on stderr or as a
//! snapshot another thread polls, plus a flag to stop the search early.

use crate::sim::clock;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_millis(100);

/// Where the search currently is.
#[derive(Clone, Copy, Default)]
pub struct Snapshot {
    pub pass: &'static str,
    pub explored: usize,
    /// Completion tick of the best build found so far.
    pub best: Option<u32>,
}

enum Sink {
    Hidden,
    Terminal,
    Shared(Arc<Mutex<Snapshot>>),
}

pub struct Progress {
    sink: Sink,
    last: Instant,
    cancel: Arc<AtomicBool>,
}

impl Progress {
    /// Shown only when stderr is a terminal, so redraws never reach pipes or logs.
    pub fn stderr() -> Self {
        let sink = if std::io::stderr().is_terminal() { Sink::Terminal } else { Sink::Hidden };
        Progress { sink, last: Instant::now(), cancel: Arc::default() }
    }

    /// Writes into `snapshot` and stops the search once `cancel` is set.
    pub fn shared(snapshot: Arc<Mutex<Snapshot>>, cancel: Arc<AtomicBool>) -> Self {
        Progress { sink: Sink::Shared(snapshot), last: Instant::now(), cancel }
    }

    #[cfg(test)]
    pub fn hidden() -> Self {
        Progress { sink: Sink::Hidden, last: Instant::now(), cancel: Arc::default() }
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Cheap enough to call per state, since it reads the clock only every 1024 states.
    pub fn update(&mut self, pass: &'static str, explored: usize, best: Option<u32>) {
        if matches!(self.sink, Sink::Hidden) || !explored.is_multiple_of(1024) || self.last.elapsed() < INTERVAL {
            return;
        }
        self.last = Instant::now();
        match &self.sink {
            Sink::Hidden => {}
            Sink::Terminal => {
                let best =
                    best.map_or_else(|| "no build yet".to_string(), |t| format!("best so far done at {} game time", clock(t)));
                eprint!("\r\x1b[K{pass}: {} states explored, {best}", group(explored));
                let _ = std::io::stderr().flush();
            }
            Sink::Shared(snapshot) => *snapshot.lock().unwrap() = Snapshot { pass, explored, best },
        }
    }

    pub fn clear(&self) {
        if matches!(self.sink, Sink::Terminal) {
            eprint!("\r\x1b[K");
        }
    }
}

/// `1234567` as `1,234,567`.
pub fn group(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
