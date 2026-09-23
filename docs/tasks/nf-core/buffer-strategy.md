# Stage seams, buffers and the IR plane

## Goal

Decide, once, what crosses each stage boundary — a fresh buffer or the same one —
and make the IR plane ride the new chain. Splitting the render into stages is a
memory decision as much as a structural one.

## Design

- **Two positions have to be reconciled.** `docs/spike/gpu-rendering-spike.md` measured the
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
- **The IR plane must ride through, and nothing today would notice if it didn't.**
  It is decoded onto the image and deliberately preserved-but-not-acted-on, so
  carrying it is a design commitment (CLAUDE.md: "carry it through, don't consume
  it") — with IR-based dust removal as the roadmap follow-up that would actually
  read it after the render. What makes that easy to lose is that **no current
  feature depends on the plane arriving at the end of the chain**: `--export-ir`
  writes from the *decoded* image, pre-render (which is also what makes
  `RunProfile::Convert` peak at encode rather than render), and both IR warnings are
  derived before the render too. Today's SDR render is the cautionary case — it
  drops the plane outright (`LinearImage::new(w, h, rgb, None)`) and no test, warning
  or counter reads zero because of it. So the check has to be a **positive assertion
  that the plane arrives**, not an inference from a warning still firing. Decide
  whether it travels in the stage types or stays with the orchestrator.
- **`nf-core/stage-skeleton` already made a provisional call here**, which this task
  may keep or overturn: the plane travels *in* the stage types
  (`working_image::WorkingBuffer` carries `ir`, and
  `fit_gamut::DisplayReferredImage::into_parts` returns it), and every boundary —
  entry, each stage, and the exit — **moves** the
  buffers rather than copying, so the identity stages allocate nothing. It did not
  decide `ir_verified`, which `AcesCgImage` already drops on the way in.
- Moving where a buffer lives must not move a pixel: the rayon pass set that bar
  with byte-identical output, and the same bar applies here.

## Open questions

- **What consume-and-return costs the typed boundary** — a consumed input cannot be
  re-fed to the same stage in a test, which is a real loss for stage goldens.
- **Are identity stages free?** Answered provisionally by the skeleton — they move
  rather than allocate, so yes — but only *measured* by reading the code. The open
  half is whether that survives once a stage does real work in place.

## How to Verify

- A before/after measurement on a real 5000 dpi frame: per-stage wall time and peak
  RSS against the reference build. Not a fixture — the effect is invisible at
  fixture size.
- The memory model's estimate stays slightly under measured, inside its 15%
  allowance, and a budget just under the modelled peak exits 6.
- The plane is asserted to **arrive at the end of the chain**, positively — an
  IR-carrying frame in, the same samples out of the last boundary. Both of the checks
  below pass with the plane dropped mid-chain, so neither can stand in for it.
- `--export-ir` writes the plane under the new flow, and the IR-preserved warning
  fires on an IR-carrying scan and promotes under `--strict` — these are pre-render
  facts, so they are regression checks on the flow, not evidence about the chain.
- The four CI gates pass.

## Dependencies

- [The new stage module tree](stage-skeleton.md)
