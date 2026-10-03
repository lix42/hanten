# Telemetry upload v1 contract

The wire contract between Hanten's uploader and the ingestion Worker
(`docs/telemetry-strategy.md`), live at `https://hanten-telemetry.i-70e.workers.dev/v1/events`
(`services/telemetry-ingest/`). **This directory is the field manifest**: where the
strategy's "Upload field manifest" table disagrees, this directory wins. Rust
(`src/telemetry/upload.rs` and its tests) and the Worker read the same files; neither
keeps a looser copy.

| File | What |
|---|---|
| `upload-v1.schema.json` | JSON Schema 2020-12 for the `POST /v1/events` body; `$defs/envelope` is the body minus its events' rules, `$defs/event` one event, `$defs/response` the Worker's reply |
| `requests/valid.json`, `requests/invalid.json` | request bodies the schema must accept / reject |
| `responses/valid.json`, `responses/invalid.json` | the same for responses |
| `local/panic-ready.json` | one local panic event's shape (`telemetry/panic-hook` writes it) |

Each case file is a JSON array, one case per line: `{"name", "request"}` (or
`"response"`). An invalid request also has `"expect"`: `http_400` when the envelope
is at fault, otherwise the rejection `code` the Worker returns for the bad event
(`invalid_field`, `unsupported_version`, `out_of_range`). Each invalid case differs
from a valid one in exactly the way its name says. The cases are plain JSON; edit
them by hand.

## Rules the schema holds

- The event is the local event (`telemetry::SCHEMA_VERSION` **11**) projected to its
  upload form (`to_upload_event` for a conversion, `to_upload_panic` for a panic);
  `source_schema_version` is that number and nothing else.
  Legacy local records never upload. A later local schema bump widens the Worker's
  accepted set rather than replacing 11; queued lines of an older local version are
  dropped by the uploader, never projected (`telemetry/upload`). The panic event is
  also version 11 (`SCHEMA_VERSION` says why).
- **Absent is not `unknown`.** A block the run never reached is absent (`image` before
  decode, `conversion` before the destination resolved, a stage's time before it
  completed, the clip fields unless the frame finished). `unknown` is a value the run
  reached but could not classify. JSON `null` and unknown keys are always rejected.
- `status`, `error_kind` and `exit_code` pair exactly: success is `none`/0; a failure
  is `usage` 2, `decode` 3, `unsupported` 4, `write` 5, `resource` 6, `strict` or
  `other` 1. A success ends in `finalize` with `image`, `conversion` and both clip
  fields. A conversion's stage is never `unknown`; a panic's is its active stage, or
  `unknown` when the hook cannot tell, and a panic carries `frames` and nothing of a
  conversion's.
- `image.bit_depth` is `16` or `"unknown"`: a per-pixel total (48, 64) never passes.

The Worker also enforces what a schema cannot: a body of at most 262,144 bytes,
unique `event_id`s within a batch, and its release allowlist (`release_blocked`). An
oversized body or a duplicate `event_id` is a malformed request: HTTP 400, which the
uploader quarantines and never retries. A schema-valid batch can exceed the size (100
panics with long frames), so the uploader splits batches to fit. The Worker
validates the body against `$defs/envelope` (400 on failure; an event without a valid
`event_id` fails it, since a rejection must name one), then each event against
`$defs/event`, rejecting only the bad ones. An event breaking several rules
gets the first of `unsupported_version`, `out_of_range`, `invalid_field`;
`release_blocked` is judged only for an event that passes the schema, and only
for a release the Worker blocks outright: an event from a release it does not yet
know is accepted and held out of analysis, not rejected. Only a
numeric `minimum`/`maximum` failure is `out_of_range`; a length, item-count or
pattern failure is `invalid_field`.

## Fields and where they come from

| Upload field | From the local event |
|---|---|
| `event_id` | copied |
| `event_day` | `timestamp_ms` ÷ 86,400,000, saturated at 65535 |
| `nc_version` | copied; must be a SemVer core with optional prerelease, else the event is not uploadable |
| `platform.os`, `platform.arch` | parsed from the `target` triple |
| `platform.cpu_bucket` | `cpu_count`, down to a power of two, `64_plus` above |
| `stage`, `outcome.status`, `error_kind`, `exit_code` | copied |
| `outcome.warning_bucket` | `warnings`: `0`, `1`, `2_3`, `4_plus` |
| `outcome.clipped_fraction_bucket` | `clipped` ÷ `total_samples` against 0.1 %, 1 %, 10 % |
| `outcome.non_finite` | `non_finite` > 0 |
| `timing_ms.*` | rounded whole ms, saturated at 86,400,000 |
| `image.format`, `ir_present` | copied |
| `image.megapixels_tenths` | `width × height`, rounded to 0.1 MP, saturated at 100,000 |
| `image.input_size_bucket` | `input_bytes`: `lt_8_mib` … `512_plus_mib` |
| `conversion.encoding` | the destination's row in `destination::ROWS` |
| `conversion.film_base_source` | the kind only: `region` or `explicit` |
| `conversion.ir_exported` | `timing_ms.ir_export` present |

Never uploaded: `params_hash`, exact dimensions, byte sizes and timestamps, the
target triple, film-base coordinates or values, the destination's axes,
`output_depth`, and any text. New fields are denied until this directory, the
projection, the Worker and the privacy tests change together, and a manifest change
needs a consent-version bump (`telemetry/upload`).

## The local panic event

`local/panic-ready.json` is one compact JSON object and a newline, at most 16 KiB:
`schema_version`, `event_id`, `event: "panic"`, `command`, `timestamp_ms`,
`nc_version`, `target`, `cpu_count`, `stage`, `frames`. The hook writes it as
`panic-ready-<event_id>.json` in the queue's spool (`telemetry::panic`). Its upload
form is the `panic` case in `requests/valid.json`, made by `to_upload_panic`: the
envelope fields as for a conversion, `stage`, and `frames` copied. A frame the hook
could not have written makes the whole event not uploadable.
