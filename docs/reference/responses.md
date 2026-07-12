# Responses

The CLI writes operational diagnostics to stderr and exits nonzero for configuration, provider, safety/store, or sink failures that are surfaced by the selected operation.

`check` succeeds only after parsing and resolving all applicable configuration references. A malformed identifier, unknown provider, unsupported feature, or unsafe event/outbox combination is an error.

`--once` reports a refresh summary:

```text
refresh complete: attempted=3 succeeded=2 failed=1
```

Accepted rates contain decimal-string `rate`, provider identity, `source_updated_at` when supplied, local `observed_at`, and derived `expires_at`. Consumers must read only rows whose `expires_at` is still in the future.

Safety events use these types: `oracle.rate_anomaly`, `oracle.rate_quarantined`, `oracle.rate_rejected`, `oracle.provider_failed`, and `oracle.refresh_failed`. Reasons include bounds, source age, change, provider spread, missing baseline, provider errors, and parse errors.
