use crate::engine::Oracle;
use crate::store::{OutboxStore, RateStore};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Global shutdown flag. Call [`request_shutdown`] to initiate graceful
/// shutdown (e.g., from a signal handler).
static SHUTDOWN: std::sync::LazyLock<Arc<AtomicBool>> =
    std::sync::LazyLock::new(|| Arc::new(AtomicBool::new(false)));

/// Request graceful shutdown of the oracle loop.
///
/// Call this from a SIGTERM or SIGINT handler. The loop will complete the
/// current refresh cycle and exit cleanly.
pub fn request_shutdown() {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

fn is_shutdown() -> bool {
    SHUTDOWN.load(Ordering::Relaxed)
}

/// Runs the oracle in a continuous loop at the configured refresh interval.
///
/// Blocks the calling thread until [`request_shutdown`] is called.
/// Wakes at `min(refresh_secs, dispatch_interval_secs)` and runs each
/// operation (refresh, outbox dispatch) only when its interval has elapsed.
///
/// The [`Oracle`] already reads previous rates from the store on each
/// `refresh_asset` call, so no state needs to be rebuilt between iterations.
pub fn run_loop<S>(oracle: &mut Oracle<S>)
where
    S: RateStore + OutboxStore,
{
    let refresh_interval = Duration::from_secs(oracle.config().oracles.refresh_secs);
    let outbox_enabled = oracle.config().outbox.enabled;
    let dispatch_interval = Duration::from_secs(oracle.config().outbox.dispatch_interval_secs);

    // Perform an immediate first refresh so rates are available without
    // waiting for the first interval tick.
    match oracle.run_once() {
        Ok(summary) => {
            crate::info!(
                "initial refresh: {} attempted, {} succeeded, {} failed",
                summary.attempted,
                summary.succeeded,
                summary.failed,
            );
        }
        Err(err) => {
            crate::error!("initial refresh error: {err}");
        }
    }

    // Also dispatch outbox immediately if enabled, to clear any stale
    // deliveries left over from a previous run.
    if outbox_enabled {
        match oracle.dispatch_outbox(50) {
            Ok(summary) if summary.attempted > 0 => {
                crate::info!(
                    "initial outbox: {att} attempted, {del} delivered, {fail} failed, {dead} dead",
                    att = summary.attempted,
                    del = summary.delivered,
                    fail = summary.failed,
                    dead = summary.dead,
                );
            }
            Ok(_) => { /* no pending deliveries */ }
            Err(err) => {
                crate::error!("initial outbox dispatch failed: {err}");
            }
        }
    }

    let mut last_refresh = std::time::Instant::now();
    let mut last_dispatch = std::time::Instant::now();

    loop {
        if is_shutdown() {
            crate::info!("shutdown signal received, draining...");
            break;
        }

        let now = std::time::Instant::now();

        // Run refresh if the refresh interval has elapsed
        if now.duration_since(last_refresh) >= refresh_interval {
            match oracle.run_once() {
                Ok(summary) => {
                    crate::info!(
                        "refresh: {} attempted, {} succeeded, {} failed",
                        summary.attempted,
                        summary.succeeded,
                        summary.failed
                    );
                }
                Err(err) => {
                    crate::error!("refresh error: {err}");
                }
            }
            last_refresh = std::time::Instant::now();
        }

        // Dispatch outbox if enabled and the dispatch interval has elapsed
        if outbox_enabled {
            let now = std::time::Instant::now();
            if now.duration_since(last_dispatch) >= dispatch_interval {
                match oracle.dispatch_outbox(50) {
                    Ok(summary) if summary.attempted > 0 => {
                        crate::info!(
                            "outbox: {att} attempted, {del} delivered, {fail} failed, {dead} dead",
                            att = summary.attempted,
                            del = summary.delivered,
                            fail = summary.failed,
                            dead = summary.dead,
                        );
                    }
                    Ok(_) => { /* no pending deliveries */ }
                    Err(err) => {
                        crate::error!("failed to dispatch outbox: {err}");
                    }
                }
                last_dispatch = std::time::Instant::now();
            }
        }

        if is_shutdown() {
            crate::info!("shutdown signal received, draining...");
            break;
        }

        // Sleep for the shorter of refresh and dispatch intervals, but at least 1 second
        let sleep_duration = if outbox_enabled {
            let next_refresh = refresh_interval.saturating_sub(last_refresh.elapsed());
            let next_dispatch = dispatch_interval.saturating_sub(last_dispatch.elapsed());
            std::cmp::min(next_refresh, next_dispatch)
        } else {
            refresh_interval.saturating_sub(last_refresh.elapsed())
        };
        let sleep = std::cmp::max(sleep_duration, Duration::from_secs(1));
        std::thread::sleep(sleep);
    }

    crate::info!("oracle stopped");
}
