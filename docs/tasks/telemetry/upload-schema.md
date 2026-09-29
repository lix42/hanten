# Telemetry upload schema v1

## Goal

Define the privacy-minimized upload event: a typed `UploadEvent` produced only
through a pure `to_upload_event(&LocalEvent)` projection of the local event schema
that [`telemetry/schema-v2`](schema-v2.md) ships, plus the checked-in upload-v1
JSON Schema and canonical valid/invalid request/response corpus that the Worker
([`ingestion-service`](ingestion-service.md)) and the uploader
([`upload`](upload.md)) both consume.

Split out of `telemetry/schema-v2` on 2026-09-28 (that task keeps the local events
and their emission). The contract is [`docs/telemetry-strategy.md`](../../telemetry-strategy.md)
as amended; its upload manifest is superseded by the one below.

## Approved manifest changes (user, 2026-09-28)

Against the strategy's "Upload field manifest":

- **`source_schema_version`** is the local event schema that `schema-v2` ships
  (`telemetry::SCHEMA_VERSION` 10) and nothing older; legacy records never upload.
- **`event_id`** is copied from the local event, which carries it from the moment
  it is written. The projection mints nothing and needs no injected ID.
- **`stage`** is a `crate::stage::StageKind` wire name, or one of `parse`, `setup`,
  `preflight`, `finalize` (the local event's `stage` values), or `unknown`, which
  only the projection writes (no local event has it).
- **`timing_ms`** has one optional field per `StageKind` plus `total`.
- **`error_kind`** gains `resource` (exit 6, the memory preflight); `exit_code`
  becomes `0..=6`.
- **`conversion.algorithm`** is dropped: one reconstruction remains.
- **`conversion.output_depth` / `output_mode`** are replaced by one
  `conversion.encoding` enum: `film_master`, `sdr_tiff`, `hdr_linear_tiff`,
  `hdr_coded_tiff`, `avif`, `gain_map_jpeg`.
- **`conversion.film_base_source`** is added as the kind only: `auto`, `region`,
  `explicit` — never its coordinates or values.

Everything else in the strategy's manifest, forbidden-data list, relational rules
and corpus requirements stands (bucketing, platform/CPU buckets, bit-depth rule,
panic frame grammar, the envelope and response shapes).

## Known

- The local event, its `event_id` and its `stage`/`error_kind` enums exist once
  `schema-v2` lands; bucketing and rounding happen only here, so the local record
  keeps exact values.
- The canonical local `panic-ready` fixture belongs to this corpus
  ([`panic-hook`](panic-hook.md) and `upload` consume it).

## Open

- **Rust-side JSON Schema validation** needs a validator (e.g. `jsonschema` as a
  dev-dependency, default features off); check its current docs and dependency
  weight first.
- **The local panic event's shape** — this corpus fixes it; `panic-hook` emits it.
- **Where the corpus lives** so the Worker can read it without a Rust build.

## How to Verify

- Upload-v1 snapshot tests (full and minimal).
- Hostile local values — paths, filenames, usernames, newlines, error messages,
  recipe values, exact dimensions/sizes/timestamps, `params_hash` — never appear in
  a serialized `UploadEvent`; no persistent or cross-event correlation field exists.
- The schema accepts every canonical valid example and rejects every invalid
  boundary/enum/string/envelope example, each invalid status/error/exit pairing,
  a wrong `source_schema_version`, and 48/64 bit-depth totals — in Rust, from the
  same files the Worker reads.

## Dependencies

- [Telemetry event schema v2](schema-v2.md) — the local event this projects.
