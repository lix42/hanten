# Third-party notices

## Gain maps

This product includes Gain Map technology under license by Adobe.

The gain-map JPEG destination writes ISO 21496-1 metadata with nc's own code
(`src/io/iso_gain_map.rs`) and encodes JPEG with the pure-Rust `jpeg-encoder` crate;
no libultrahdr or libjpeg-turbo code is linked. The Adobe HDR Gain Map license text
this notice answers to shipped with libultrahdr until `nf-core/default-flip` removed
that dependency; a binary release must source it again (the release task tracks it).

## libaom (removed)

The PQ / HLG AVIF destinations, and with them libaom and its Alliance for Open Media
Patent License, were removed on 2026-10-01; no AV1 code is linked. The license
summary and the pending AOM patent review that applied while they shipped are in
git history, and `docs/design/avif-removal.md` says what bringing AVIF back
restores.

## Binary distribution license bundle

A binary release must package this notice file together with the Adobe HDR Gain Map
license text (see "Gain maps").
