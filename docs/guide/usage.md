# Usage

```sh
# validate only; no provider fetch or store open
oracles --config Config.toml check

# fetch once, decide, persist, and exit
oracles --config Config.toml --once

# refresh continuously at oracles.refresh_secs
oracles --config Config.toml

# liveness response; does not load configuration
oracles ping
```

The worker records provider and refresh failures as events when events are enabled. With `fail_fast = false`, a failed asset does not stop the cycle. In simple event mode matching sinks are called after the event is stored; in outbox mode a dispatcher retries pending rows until delivered or dead.

The logger exports `trace!`, `debug!`, `info!`, `warn!`, and `error!`. The module re-export is named `log_warn` because `warn` conflicts with Rust's built-in `#[warn(...)]` attribute; use `crate::warn!()` directly or import `logging::log_warn as warn`.
