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

- **`source_schema_version`** is the local event schema this task ships
  (`telemetry::SCHEMA_VERSION` 11, see Decisions) and nothing older; legacy records
  never upload.
- **`event_id`** is copied from the local event, which carries it from the moment
  it is written. The projection mints nothing and needs no injected ID.
- **`stage`** is a `crate::stage::StageKind` wire name, or one of `parse`, `setup`,
  `preflight`, `finalize` (the local event's `stage` values), or `unknown`, which
  only a panic carries, when the hook cannot tell its active stage (that event
  arrives with `telemetry/panic-hook`).
- **`timing_ms`** has one optional field per `StageKind` plus `total`.
- **`error_kind`** gains `resource` (exit 6, the memory preflight); `exit_code`
  becomes `0..=6`.
- **`conversion.algorithm`** is dropped: one reconstruction remains.
- **`conversion.output_depth` / `output_mode`** are replaced by one
  `conversion.encoding` enum: `film_master`, `sdr_tiff`, `hdr_linear_tiff`,
  `hdr_coded_tiff`, `avif`, `gain_map_jpeg`.
- **`conversion.film_base_source`** is added as the kind only: `region`, `explicit`
  (see Decisions) — never its coordinates or values.

## Decisions (user, 2026-09-28, at start)

- **Local schema 11** adds `outcome.total_samples`: the clip fraction needs the
  encoder's denominator, and a gain map counts two renditions, so the image's
  dimensions are not it. Nothing had uploaded, so the bump is free.
- **`film_base_source` drops `auto`**: `film-base/holder-masked-measurement` retired
  it; `effective_area` is unreachable and makes an event not uploadable.
- **The projection takes the typed `TelemetryEvent`**; reading JSONL lines back into
  events is `telemetry/upload`'s.
- **The contract lives in `contracts/telemetry/upload-v1/`**, whose README is now the
  field manifest.
- Implementation choices: absent (not reached) is distinct from `unknown` (reached,
  unclassifiable), so never-written `unknown` members are gone; the validator is
  `boon` (a dev-dependency), because `jsonschema` bumps locked runtime crates.

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

All resolved (see Decisions): the validator is `boon`; the local panic shape is
`contracts/telemetry/upload-v1/local/panic-ready.json`; the corpus lives beside it.

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
