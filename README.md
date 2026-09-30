# Hanten

A command-line tool that converts film **negative** scans into **positive**
images. The binary is `hanten`; `nc` remains the internal name of the crate and
of every versioned identifier (see `CLAUDE.md` for the boundary).

It reads high-bit-depth scanner files (SilverFast HDR/HDRi first), runs a
deterministic negative→positive pipeline in a 32-bit float linear working space —
a fixed decode, then scene correction, the look, fit range and fit gamut — and
writes a 16-bit Display P3 TIFF by default. Destination flags select the rest: SDR
TIFF in Adobe RGB, an HDR gain-map JPEG (ISO 21496-1), a 10-bit HDR AVIF (PQ or HLG),
an HDR TIFF (PQ, HLG or linear float), or the unrendered linear ACEScg film master.

## Design goal: built for agents

Film conversion has many knobs — film-base estimation, density, white balance,
tone, gamma, color management. The core idea here is that **every parameter is a
CLI flag** and the tool is deterministic and scriptable (JSON recipes in, JSON
reports out), so an automated agent — or a human — can drive the whole conversion
reproducibly.

This is *not* about using AI/ML to process images. The pipeline is a
physics-based deterministic core; any future ML assistance stays optional and
around the edges.

## Status

The Step-1 TIFF converter is implemented, with post-MVP pipeline, display-output,
and hardening work tracked in the task roadmap.

- [`docs/using-nc.md`](docs/using-nc.md) — **how to use `hanten`**: the
  measure → freeze a recipe → apply workflow, recipes, destinations, exit codes.
- [`docs/design-spec.md`](docs/design-spec.md) — full design (architecture,
  pipeline, CLI surface, parameters).
- [`docs/TASKS.md`](docs/TASKS.md) — the build plan and dependency graph.
- [`docs/negative-convertor-research-report.md`](docs/negative-convertor-research-report.md)
  — background research.
- [`docs/spike/gpu-rendering-spike.md`](docs/spike/gpu-rendering-spike.md) — where `hanten` spends
  its time, and why it multithreads every per-pixel stage and the AV1 encoder on the
  CPU rather than rendering on a GPU; revisit for an interactive or browser app.

## Usage (current CLI)

```sh
# A roll: measure what it shares — the film base from its unexposed frame, and its
# white balance, white and exposure over its frames — once, into a recipe; then convert it.
hanten measure-roll frames/*.tif --unexposed unexposed.tif --leader leader.tif --out roll.json
hanten roll frames/*.tif --out-dir out/ --params roll.json

# The film base (Dmin) alone, from the unexposed frame (the median over the frame,
# holder cut away) — or, with no unexposed frame, --base-region X,Y,W,H on a region
# of unexposed film. --out writes it as a recipe for --params.
hanten measure-base unexposed.tiff --out base.json

# Convert a negative scan to a positive 16-bit Display P3 TIFF. Every conversion
# must state where the film base comes from — there is no default, because Dmin sets
# both the black point and the colour balance.
hanten convert in.tiff -o out.tiff --film-base 0.92,0.55,0.42

# Half a stop brighter, with a little more contrast.
hanten convert in.tiff -o out.tiff --film-base 0.92,0.55,0.42 \
  --exposure 0.5 --contrast 1.3

# HDR: a gain-map JPEG (the suffix is completed from the destination), or PQ AVIF.
hanten convert in.tiff -o out --film-base 0.92,0.55,0.42 --range hdr
hanten convert in.tiff -o out --film-base 0.92,0.55,0.42 --transfer pq --container avif

# Inspect a scan and emit machine-readable JSON.
hanten inspect in.tiff --report json
```

See the design spec for the complete command and parameter reference.

## Building

The Rust build also compiles libaom (the AV1 encoder behind the PQ / HLG AVIF
destinations) statically from the `libaom-sys` crate's vendored source. A fresh
build machine needs CMake, C and C++ compilers, and NASM (for libaom SIMD on
supported targets). No network access is needed for the native build. For example:

```sh
# Debian/Ubuntu
sudo apt-get install build-essential cmake nasm

# macOS with Homebrew (Xcode Command Line Tools are also required)
brew install cmake nasm
```

Runtime deployment does not require a separate libjpeg, libaom or libavif
installation — Hanten writes the JPEG, gain-map and AVIF containers itself and
statically links every codec. The Adobe Gain Map notice and the libaom / Alliance for
Open Media patent-license summary are in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md), which also lists the license
files a binary release must include.

## License

TBD.

Third-party notices and license terms are collected in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
