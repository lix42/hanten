# AVIF row multithreading

**Done:** 2026-09-16 — see `docs/progress/output.md`. No `pipeline_version` bump: the presets are not the default.

## Goal

Let libaom use row multithreading with a **fixed thread count ≥ 2**, cutting the
`hdr-pq`/`hdr-hlg` encode from ~2.8 s to ~0.6 s on a 16.4 MP scan while keeping the
output reproducible ([gpu-rendering-spike](../../gpu-rendering-spike.md), Experiment 2).

## Design

`io/avif.rs` **pinned** `g_threads = 1` and `AV1E_SET_ROW_MT = 0` as part of the
determinism contract (this task changed that; the paragraph describes the starting
point). The spike measured that with row-mt **on**, 2, 4 and 8
threads produce identical bytes, run to run; only the 1-thread case differs,
because libaom treats one thread as row-mt off. So the contract becomes: row-mt on,
thread count pinned to a constant ≥ 2 (not derived from the machine), tiles still
off.

- Choose the constant (8 is the measured 5x; it must not depend on core count).
- This **changes shipped bytes** for `hdr-pq`/`hdr-hlg` (+0.11% size on the spike
  frame). `pipeline_version` bumps only when the *default* render changes
  (design-spec §9) and these presets are not the default, so no bump; record the
  change as an addendum to the current baseline report instead.
- Re-check `cq_level`/codec-bound assertions that were pinned by equality against
  dav1d/avifdec still hold on the new codestream.
- CI test: encode a fixture at 2 and at 8 threads on Linux and assert equal hashes,
  so a libaom bump that breaks thread-count independence fails loudly.
- `RunProfile::HdrAvif` staging may change with worker threads; re-measure.

Open: whether `hdr-avif-windows-packaging` should inherit the same test.

## How to Verify

- Four gates green; the new equal-hash test runs in CI on both OSes.
- On the spike frame: encode stage ~2824 → ~570 ms; the 2-, 4- and 8-thread
  outputs are byte-identical to each other; decode with dav1d/avifdec is bit-exact.
- `docs/using-nc.md` unchanged unless a user-visible note is warranted (no flag
  is added).

## Dependencies

- [HDR AVIF output](hdr-avif-output.md)
- [Conversion versioning & baseline comparison](../core/conversion-versioning.md)
