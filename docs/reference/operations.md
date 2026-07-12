# Operations

Use SQLite for a local durable worker or PostgreSQL for a shared deployment. Enable `migrate = true` to create the rates, events, and outbox schema; externally managed schemas must include the unique `(asset_id, quote, provider)` index for upsert mode.

For notifications, choose `mode = "outbox"`, keep `events.record = true`, and configure retry limits. The delivery state is `pending`, `delivered`, or `dead`; application owners should monitor dead rows and stale rates.

Run the project checks:

```sh
cargo fmt --all --check
cargo check --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Operationally, treat a missing fresh rate as unavailable rather than reusing an expired value. Keep provider credentials and webhook tokens in environment variables, set conservative bounds and source-age limits, and use multiple feeds with consensus for higher-value decisions.
