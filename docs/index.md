---
layout: home

hero:
  name: Oracles
  text: Durable cryptocurrency-rate decisions
  tagline: A Rust worker and library for fetching, validating, and storing rates.
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: GitHub
      link: https://github.com/melonask/oracles

features:
  - title: Provider-driven
    details: Static and HTTP JSON providers are configured per asset feed.
  - title: Safety-first
    details: Bound, freshness, change, bootstrap, and consensus checks yield explicit decisions.
  - title: Durable delivery
    details: SQLite or PostgreSQL stores accepted rates, audit events, and optional outbox deliveries.
---

Oracles is stateless at runtime: the configured store is the source of truth.
