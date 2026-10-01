# Drop AVIF output

**Done:** 2026-10-01 — see `docs/progress/output.md`. The decision, and how to bring
AVIF back, is [`docs/design/avif-removal.md`](../../design/avif-removal.md).

## Goal

Remove the `hdr / pq|hlg / bt2020 / avif` destinations and the `libaom-sys`
dependency, so the native build needs no CMake, C++ or NASM. The PQ/HLG TIFFs keep
the same Rec.2100 signal, and the gain-map JPEG stays the compact HDR file.

## Design

- Delete `io::avif`, `Container::Avif`, `Encoding::HdrAvif`, the report's `avif`
  block, `RunProfile::Avif` and the AVIF-only BT.2020 Y'CbCr matrix.
- Refuse `avif` by name wherever it can be stated (flag, recipe value, `.avif`
  suffix, `--output-preset hdr-pq|hdr-hlg`), with the TIFF as the remedy and no
  alias.
- Keep `"avif"` in the telemetry upload contract for older clients.
- No `pipeline_version` change: AVIF was never the default render.

## How to Verify

- The four Rust gates, the `nctool` suite and CI green without `cmake`/`nasm`
  installed.
- `--container avif`, a recipe's `"container": "avif"`, `-o out.avif` and a `roll`
  over that recipe each exit 2 naming the removal and write nothing.

## Dependencies

- [HDR AVIF output](hdr-avif-output.md)
