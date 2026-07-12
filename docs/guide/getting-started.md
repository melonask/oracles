# Getting started

Install the default SQLite build, copy the example, and validate it before it performs I/O:

```sh
cargo install oracles
cp Config.example.toml Config.toml
oracles --config Config.toml check
```

Use `--features full` when the configuration enables PostgreSQL, Telegram, or webhook sinks. Provider secrets are environment variables, never literals in the config. For a local no-network run, configure the static provider with a feed `params.rate`.

Run one refresh:

```sh
oracles --config Config.toml --once
```

Example output:

```text
refresh complete: attempted=2 succeeded=2 failed=0
```

See [Configuration](/guide/configuration) for production settings.
