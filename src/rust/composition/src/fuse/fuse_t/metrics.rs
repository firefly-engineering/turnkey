//! FUSE operation metrics
//!
//! Tracks per-operation call counts, total time, and max latency.
//! Periodically logs a summary so we can identify bottlenecks.

#![cfg(target_os = "macos")]
#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Per-operation counters
struct OpMetric {
    count: AtomicU64,
    total_us: AtomicU64,
    max_us: AtomicU64,
}

impl OpMetric {
    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            total_us: AtomicU64::new(0),
            max_us: AtomicU64::new(0),
        }
    }

    fn record(&self, elapsed: Duration) {
        let us = elapsed.as_micros() as u64;
        self.count.fetch_add(1, Ordering::Relaxed);
        self.total_us.fetch_add(us, Ordering::Relaxed);
        self.max_us.fetch_max(us, Ordering::Relaxed);
    }

    fn snapshot(&self) -> (u64, u64, u64) {
        (
            self.count.load(Ordering::Relaxed),
            self.total_us.load(Ordering::Relaxed),
            self.max_us.load(Ordering::Relaxed),
        )
    }
}

macro_rules! define_ops {
    ($($name:ident),* $(,)?) => {
        pub(crate) struct Metrics {
            $($name: OpMetric,)*
            last_report: std::sync::atomic::AtomicU64,
        }

        impl Metrics {
            const fn new() -> Self {
                Self {
                    $($name: OpMetric::new(),)*
                    last_report: AtomicU64::new(0),
                }
            }

            pub fn log_summary(&self) {
                use log::info;
                info!("=== FUSE metrics ===");
                $(
                    let (count, total_us, max_us) = self.$name.snapshot();
                    if count > 0 {
                        let avg = total_us / count;
                        info!(
                            "  {:12} | calls: {:8} | total: {:8}ms | avg: {:6}µs | max: {:6}µs",
                            stringify!($name), count, total_us / 1000, avg, max_us
                        );
                    }
                )*
            }
        }

        $(
            pub(crate) fn $name(start: Instant) {
                METRICS.$name.record(start.elapsed());
                maybe_report();
            }
        )*
    };
}

define_ops! {
    getattr,
    readdir,
    open,
    read,
    readlink,
    statfs,
    access,
    opendir,
    releasedir,
    mkdir,
    unlink,
    rmdir,
    create,
    write,
    truncate,
    chmod,
    rename,
    symlink,
}

static METRICS: Metrics = Metrics::new();

/// Report interval in seconds
const REPORT_INTERVAL_SECS: u64 = 30;

fn maybe_report() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let last = METRICS.last_report.load(Ordering::Relaxed);
    if now - last >= REPORT_INTERVAL_SECS {
        if METRICS
            .last_report
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            METRICS.log_summary();
        }
    }
}

/// Force a metrics report (e.g., on unmount)
pub(crate) fn report() {
    METRICS.log_summary();
}
