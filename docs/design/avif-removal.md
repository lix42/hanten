# AVIF output removed

**Decided:** 2026-10-01 (user). **Task:** `output/drop-avif`. **Last commit with
AVIF:** `d6f5693` on `main` (`git log --diff-filter=D -- src/io/avif.rs` finds the
removal).

Hanten no longer writes AVIF. The `hdr / pq|hlg / bt2020 / avif` destinations are
gone, and so are their encoder (`io::avif`) and the `libaom-sys` dependency. This
document records why, what is left, and how to bring AVIF back.

## Why

- **The build cost belongs to one opt-in output.** `libaom-sys` compiles libaom
  from source with CMake and a C++ compiler, and on x86/x86_64 libaom's CMake
  refuses to configure without NASM (or yasm) for its SIMD. Nothing else in the
  tree needs CMake, C++ or NASM. Without AVIF the native build is one C compiler
  for lcms2 (and `ring`, which assembles with it).
- **AVIF was never a default.** It was reached only by `--container avif` or an
  `.avif` output path.
- **No HDR signal is lost.** The PQ/HLG TIFFs carry the same rendition the AVIF
  coded: the same `hdr::encode_transfer` output, stored exactly as full-range 16-bit
  codes where the AVIF quantized to 10 bits and compressed it. The float TIFF and
  the gain-map JPEG are unaffected.
- **Open work went with it.** The Windows build (`output/hdr-avif-windows-packaging`,
  which needed MSVC, CMake and NASM) and the counsel review of the AOM patent grant
  were both pending only because of libaom.

The removal does **not** rest on the ecosystem having moved. As of 2026-10-01 the
facts behind the original encoder choice (`output/hdr-avif-output`) still held:
`libavif-sys` 0.17.0 bundles libavif 1.0.4, which predates the `MA1A` brand;
`avif-serialize` 0.8.9 hardcodes its brand list; `libaom-sys` 0.17.2 is still libaom
3.11.0; `rav1e` is at 0.8.1.

## What it costs

The only **compact BT.2020 PQ/HLG file** a browser or OS viewer shows as HDR
directly. What remains:

| need | destination |
|---|---|
| the Rec.2100 signal, exact | `--transfer pq` / `--transfer hlg` → 16-bit TIFF (large; only a CICP-aware colour-managed reader presents it as HDR) |
| the HDR master | `--transfer linear --gamut bt2020` → 32-bit float TIFF |
| a compact HDR file that displays | `--range hdr` → gain-map JPEG (Display P3 or sRGB base, not BT.2020) |

## Compatibility

- **`avif` is refused by name, never as a typo**, wherever it can be stated: the
  `--container` flag, the recipe key `output.display.container` (so a report or
  sidecar from an AVIF run replays into the refusal), an `.avif` output path (which
  would otherwise be read as the stem of `out.avif.tiff`), and `--output-preset
  hdr-pq|hdr-hlg`, whose counterpart is now the TIFF. One table holds it:
  `destination::Container`'s `Axis::REMOVED` entry and `destination::AVIF_REMOVED`.
  There is no alias, because a TIFF is not what an AVIF recipe asked for.
- **No `pipeline_version` change.** AVIF was never the default render, and no
  remaining destination's pixels moved.
- **Telemetry.** The upload contract (`contracts/telemetry/upload-v1`) keeps
  `"avif"` in `conversion.encoding`, so uploads from older builds stay valid; this
  build never sends it. A spooled local event from
  an older build that names the `avif` container no longer parses, so the uploader
  quarantines it like any other line it cannot project.
- **`nctool` keeps its AVIF knowledge.** It drives other builds (`--build`, the
  reference build) and reads their reports, and those builds can write AVIF.
- **The colorimetry catalogue lost one entry**, `BT2020_NCL_RGB_TO_YCBCR` (and
  `derive::ycbcr_from_luma`): the BT.2020 Y'CbCr matrix existed only for AVIF's
  `matrix_coefficients = 9`. `BT2020_LUMA`, which the HDR render uses, stays.

## Bringing it back

### Pick the encoder first

Hanten writes the AVIF container itself, so it needs only an AV1 encoder that does
**10-bit 4:4:4** (High Profile) and is **byte-deterministic for a pinned
configuration**. Libavif is not needed, and adding it would not remove the codec
build: it still needs libaom or rav1e underneath.

| option | build cost | notes |
|---|---|---|
| `libaom-sys`, vendored (what shipped) | CMake, C++, NASM on x86/x86_64 | Revert this removal. All the calibration below applies as is. |
| `libaom-sys` against a prebuilt libaom (`LIB_AOM_STATIC_LIB_PATH`, `LIB_AOM_INCLUDE_PATH`, `LIB_AOM_PKG_CONFIG_PATH`) | none at `cargo build` | `Cargo.lock` no longer pins the codec, so byte identity becomes a property of the machine. |
| `rav1e` | pure Rust; its `asm` feature needs NASM on x86_64 | Without `asm`, no NASM but much slower. Not yet checked: 4:4:4 at 10 bits through its API, and thread-count determinism. Every codec bound and the memory fit must be re-measured. |
| SVT-AV1 | — | Not an option as far as is known: it encodes 4:2:0 only. |

Whichever is chosen, consider an **optional Cargo feature** (`avif`) so the default
build stays free of the codec. The destination table then needs a row this build
may lack, refused with a message naming the feature, and CI must build both ways.

### Restore the code

`git revert` of the removal commit is the shortest path while little has moved.
Otherwise restore from `d6f5693`, e.g. `git show d6f5693:src/io/avif.rs`. The
removal touched:

- `Cargo.toml` — `libaom-sys` with `av1_encoder`, and as a dev-dependency with
  `av1_decoder` for the round-trip tests (resolver v3 keeps that out of the shipped
  binary).
- `src/io/avif.rs` and `io/mod.rs` — the encoder and the MIAF container.
- `src/destination.rs` — `Container::Avif`, `Encoding::HdrAvif`, the two rows. Drop
  the `Axis::REMOVED` entry, `AVIF_REMOVED` and `Container::removed_suffix` (with the
  check in `cli::resolve_output_path`) once `avif` parses again.
- `src/cli.rs` — the report's `avif` block (`AvifResult`) and its roll-frame twin,
  the encode dispatch, `run_profile`, `primary_depth` (`u10`), and the
  `--output-preset hdr-pq|hdr-hlg` counterparts.
- `src/pipeline/memory.rs` — `RunProfile::Avif` and its staging constant (below).
- `src/pipeline/colorimetry/` — `BT2020_NCL_RGB_TO_YCBCR`, its audit entry and
  tests; regenerate `derived-artifacts.txt`.
- `src/telemetry/upload.rs` — the `"avif"` encoding (the schema still accepts it).
- Tests in `tests/pipeline.rs` and the modules above; docs (`using-nc.md`,
  design-spec §5 and §9, `README.md`, `THIRD_PARTY_NOTICES.md` with the libaom
  licence and the AOM patent review, `CLAUDE.md`); CI's `cmake nasm` install steps;
  and reopen `output/hdr-avif-windows-packaging`.

### What the shipped encoder had settled

Restoring libaom keeps all of this. A different encoder must re-establish each
point.

- **`av1C` is parsed from the codestream's sequence header**, never taken from the
  encoder: `AV1E_GET_SEQ_LEVEL_IDX` reports the *target* level (31 = unset).
- **`MA1A` is written only inside the AVIF v1.2 Advanced Profile limits**: at most
  35,651,584 pixels, 16,384 wide and 8,704 high, and level ≤ 6.0 (`seq_level_idx`
  16). Outside them the file is a valid general-brand AVIF, and the report says why.
  libaom writes `seq_level_idx` 31 ("maximum parameters") for a 74.6 MP scan.
- **CICP** is `9/16/9` for PQ and `9/18/9` for HLG, full range. `clli` is written
  for PQ only; HLG is display-referred.
- **Pinned encoder settings**, part of the byte-determinism contract rather than
  knobs: `cpu-used` 6, constant quality `cq_level` 8, tiling off, row-mt on with **8**
  threads. libaom's row-mt output was identical for every thread count from 2
  upward on libaom 3.11.0, measured rather than documented. One thread switches
  row-mt off and changes the bytes.
- **Quality**, measured on an 18.66 MP scan (file size) and a 256x64 test field
  decoded by `avifdec`/dav1d (worst per-plane code error out of 1023):

  | `cq_level` | file | max err | RMS |
  |---|---|---|---|
  | 0 | 20.38 MiB | 0 | 0.000 |
  | 8 | 0.99 MiB | 10 | 0.85 |
  | 12 | 0.35 MiB | 14 | 1.24 |
  | 20 | 0.07 MiB | 20 | 1.91 |

  `cq_level` 0 is mathematically lossless.
- **Memory.** Encode staging was one lumped 48 B/px term over the retained f32
  rendition. It was fitted on two `hdr-pq` runs with an explicit base: 18.66 MP →
  1,472,397,312 B and 74.65 MP → 5,865,947,136 B peak RSS, i.e. 78.47 B/px with
  ~7.9 MB fixed. Both runs had one thread and row-mt off; 8 row-mt workers added
  about 10 MB at 16.4 MP. Re-fit with the current settings rather than trusting
  these.

The build history is in `docs/progress/output.md` (`hdr-avif-output`,
`avif-row-multithreading`) and in the task files of the same names.
