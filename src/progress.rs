//! A one-line search status on stderr, redrawn at most a few times a second.

use crate::sim::clock;
use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_millis(100);

pub struct Progress {
    enabled: bool,
    last: Instant,
}

impl Progress {
    /// Shown only when stderr is a terminal, so redraws never reach pipes or logs.
    pub fn stderr() -> Self {
        Progress { enabled: std::io::stderr().is_terminal(), last: Instant::now() }
    }

    #[cfg(test)]
    pub fn hidden() -> Self {
        Progress { enabled: false, last: Instant::now() }
    }

    /// Cheap enough to call per state, since it reads the clock only every 1024 states.
    pub fn update(&mut self, pass: &str, explored: usize, best: Option<u32>) {
        if !self.enabled || !explored.is_multiple_of(1024) || self.last.elapsed() < INTERVAL {
            return;
        }
        self.last = Instant::now();
        let best = best.map_or_else(|| "no build yet".to_string(), |t| format!("best so far done at {} game time", clock(t)));
        eprint!("\r\x1b[K{pass}: {} states explored, {best}", group(explored));
        let _ = std::io::stderr().flush();
    }

    pub fn clear(&self) {
        if self.enabled {
            eprint!("\r\x1b[K");
        }
    }
}

fn group(n: usize) -> String {
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
