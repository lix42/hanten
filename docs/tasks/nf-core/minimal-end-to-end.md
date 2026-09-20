# A minimal end-to-end render

## Goal

`nc convert --new-flow` reads a scan and writes a file: reconstruction → the new
stage chain → one destination. This is the milestone the whole migration is
sequenced around — it is what `nf-retire`
waits on, so it should be the *thinnest* thing that genuinely runs end to end, not the
first good-looking render.

## Design

- **Every stage may still be an identity pass.** Scene correction, look, fit range
  and fit gamut are allowed to do nothing here; what must be real is that the chain
  is wired, typed, ordered and reachable from the CLI. Behaviour is each stage epic's
  job, and filling one must not require re-plumbing.
- **One destination, chosen for least machinery.** The SDR TIFF shape is the cheapest
  — no gain map, no dual rendition, no container staging — and `nf-destinations` owns
  the rest. Pick one and say why.
- **A `RunProfile` is owed before the destination ships**, not after: the memory
  preflight gates every decoding command, and CLAUDE.md's rule is that a new preset
  calibrates its own profile (and that calibration solves across two frame sizes).
  If the chosen destination's buffer arithmetic is genuinely the same shape as an
  existing profile, confirm that by measurement rather than inheriting it.
- **The report has to say something true.** Prose that names an operation is a claim
  about the run; an identity look stage that reports "graded" is the exact defect
  CLAUDE.md records as the knob's "fifth spot". Either derive the prose from the
  resolved chain or state facts in fields.

## Open questions

- **Which destination**, and whether it is a name users will keep or a temporary one
  that `nf-destinations` renames.
- **Does the report change shape here or later?** A new chain has new stage names;
  emitting the old report shape from the new flow is cheap now and a migration later.
- **What is the pass bar?** "A file that decodes and is not obviously broken" is the
  right bar for this task. Whether it *looks* right is `nf-calibration`'s question and
  must not be smuggled in here.

## How to Verify

- `nc convert --new-flow` on a committed fixture exits 0 and writes a file that
  decodes, with the declared profile matching the pixels.
- A run without `--new-flow` is byte-identical to before — the old flow is untouched.
- Determinism: the same inputs twice produce identical bytes.
- The memory preflight admits and rejects the new destination at the expected
  boundary (a budget just under the modelled peak exits 6).
- The four CI gates pass.

## Dependencies

- [The new stage module tree](stage-skeleton.md)
- [The fixed, stock-agnostic decode](../nf-reconstruction/fixed-decode.md)
