---
title: telemetry
order: 7
---

`src/telemetry.rs` and the opt-in upload stack (schema, ingestion service,
uploader, panic hook), from the 2026-07-14 telemetry discussion: local-only
instrumentation first, remote telemetry a deliberately separate opt-in roadmap
item (design-spec §12). **Operational, never a conversion knob** — nothing here
may perturb deterministic image output.
