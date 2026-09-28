# Third-party notices

## Gain maps

This product includes Gain Map technology under license by Adobe.

The gain-map JPEG destination writes ISO 21496-1 metadata with nc's own code
(`src/io/iso_gain_map.rs`) and encodes JPEG with the pure-Rust `jpeg-encoder` crate;
no libultrahdr or libjpeg-turbo code is linked. The Adobe HDR Gain Map license text
this notice answers to shipped with libultrahdr until `nf-core/default-flip` removed
that dependency; a binary release must source it again (the release task tracks it).

## libaom (AV1) — the PQ / HLG AVIF destinations

The `--transfer pq|hlg --container avif` destinations encode AV1 with libaom, reached through the
published `libaom-sys` crate. That crate vendors libaom's source and links it
statically, so **no libaom snapshot is checked into this repository** and the exact
version is pinned by `Cargo.lock` (`libaom-sys 0.17.2+libaom.3.11.0` at the time of
writing). The license and patent files travel with the crate source, under the
Cargo registry checkout:

- `<cargo-registry-src>/libaom-sys-<version>/vendor/LICENSE`
- `<cargo-registry-src>/libaom-sys-<version>/vendor/PATENTS`

libaom is Copyright (c) 2016, Alliance for Open Media, under the 2-clause BSD
license reproduced in that `LICENSE`. `libaom-sys` itself is BSD-2-Clause.

### AOM patent license review

libaom's `PATENTS` carries the **Alliance for Open Media Patent License 1.0**,
which grants each Licensor's Necessary Claims on a "no-charge, royalty-free,
irrevocable" basis to make, use, sell, offer for sale, import or distribute an
Implementation, subject to its § 1.2 conditions (notably the defensive-termination
and availability clauses). Distributing nc's AVIF output path therefore needs no
per-unit royalty to participating licensors.

This is a **factual summary of the shipped license text, not legal advice, and not
a completed legal review.** The HDR output spike deliberately re-homed the
"licensed normative text / legal review" gate to the encoder tasks rather than
treating it as satisfied (see `docs/spike/hdr-output-spike.md`). What is discharged here
is the *standards* half — the AVIF v1.2 and AV1 specifications are public, so the
profile, brand and CICP conformance claims were verified against their normative
text. Counsel review of the AOM patent grant before a binary release remains
outstanding and is tracked with the release task.

nc does **not** depend on libavif; the AVIF container is written by `src/io/avif.rs`
and carries no third-party code.

## Binary distribution license bundle

A binary release must package this notice file together with the Adobe HDR Gain Map
license text (see "Gain maps") and, copied out of the `libaom-sys` crate source at
the version `Cargo.lock` pins:

- libaom's `vendor/LICENSE`
- libaom's `vendor/PATENTS` (Alliance for Open Media Patent License 1.0)
