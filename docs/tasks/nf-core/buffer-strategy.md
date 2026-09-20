# Stage seams, buffers and the IR plane

## Goal

Decide, once, what crosses each stage boundary — a fresh buffer or the same one —
and make the IR plane ride the new chain. Splitting the render into stages is a
memory decision as much as a structural one.

## Design

- **Two positions have to be reconciled.** `docs/gpu-rendering-spike.md` measured the
  pipeline and concluded the seams are the *existing typed boundaries*
  (`FilmRgbImage`, `AcesCgImage`, `SharedDisplaySource`), "not a new one per stage";
  [the stage skeleton](stage-skeleton.md) proposes a pure function per stage. Those
  are compatible only if most stages hand back the buffer they were given.
- **The arithmetic says this is not a rounding error.** A full-frame f32 RGB buffer
  is 12 B/px — roughly 0.9 GB at 74.6 MP against a fixed 6 GiB default — so a buffer
  per stage would have `memory::preflight` refusing frames it admits today, visible
  only on real 5000 dpi scans and never on a committed fixture. Whatever is decided,
  `pipeline/memory.rs`'s model moves in the same change: nothing tests that model
  against the code, so it under-approves silently.
- **Decide in-place versus consume-and-return per stage, and say why.** The
  precedent is `color::to_output`, changed from cloning to consuming precisely
  because two live full-frame images is the design and three was the bug.
- **The IR plane must ride through.** It is decoded onto the image and deliberately
  preserved-but-not-acted-on; `--export-ir` and the strict-promotable "IR preserved
  but not used" warning both depend on it surviving to the end of the run. CLAUDE.md
  records that warning being silently suppressed on 22 of 25 real frames when a
  caller re-derived its condition instead of reading what the stage returned — the
  same shape as a plane quietly dropped at a new type boundary. Decide whether it
  travels in the stage types or stays with the orchestrator, and note that holding
  the decoded image for `--export-ir` is what makes `RunProfile::Convert` peak at
  encode rather than render.
- Moving where a buffer lives must not move a pixel: the rayon pass set that bar
  with byte-identical output, and the same bar applies here.

## Open questions

- **What consume-and-return costs the typed boundary** — a consumed input cannot be
  re-fed to the same stage in a test, which is a real loss for stage goldens.
- **Are identity stages free?** An identity pass that still allocates is the worst of
  both answers, and the skeleton ships several.

## How to Verify

- A before/after measurement on a real 5000 dpi frame: per-stage wall time and peak
  RSS against the tagged reference build. Not a fixture — the effect is invisible at
  fixture size.
- The memory model's estimate stays slightly under measured, inside its 15%
  allowance, and a budget just under the modelled peak exits 6.
- `--export-ir` writes the plane under the new flow, and the IR-preserved warning
  fires on an IR-carrying scan and promotes under `--strict`.
- The four CI gates pass.

## Dependencies

- [The new stage module tree](stage-skeleton.md)
