# Configuration

The root shared sections are `log`, `stores`, `http`, `chains`, and `assets`; package settings are under `oracles`. Unknown fields in the `oracles` namespace are rejected. `${NAME}` requires an environment value and `${NAME:-default}` supplies one.

```toml
[stores.oracles]
driver = "sqlite"
url = "sqlite://data/oracles.db"
migrate = true
max_connections = 1

[oracles]
store = "oracles"
quote = "USD"
refresh_secs = 180
stale_after_secs = 300
selection = "priority"

[oracles.safety]
compare_against = "last_accepted"
default_action = "quarantine"
max_change_pct = "50"
```

`stale_after_secs` must be at least `refresh_secs`. Stores currently use one connection. In `upsert` mode, the store keeps one rate per `(asset_id, quote, provider)` with an atomic database upsert; `append` retains history.

Providers are `static` (feed `params.rate`) or `http_json` (URL template, JSON rate path, and optional source timestamp). `priority` takes the first successful feed, `all` evaluates each successful feed, and `median` evaluates the median after consensus.

Safety produces `alert` (accept plus event), `quarantine`, `reject`, or `disable_asset`. `last_observed` requires durable event recording. Outbox mode requires event recording and writes events plus delivery rows in the decision transaction.
