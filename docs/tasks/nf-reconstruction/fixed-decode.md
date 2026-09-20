# The fixed, stock-agnostic decode

## Goal

Under `--new-flow`, reconstruction is one decode for every negative: a straight line
in density against log exposure, with the film's toe **passed through as recorded**.
Nothing in it shapes tone, nothing in it is per stock. Everything about how the
picture should look moves to rendering (design-update Part 1).

## Design

The chain is measurement → calibration → curve: divide by the measured film base,
take the log, apply the calibration gain, and exponentiate through a fixed contrast
about a fixed anchor. Every step is strictly increasing and unclamped in f32, so the
decode loses nothing by construction — which is why its settings are conventions
rather than accuracy questions.

**Everything this needs already exists as a reachable configuration.** The
exponential curve *is* the sigmoid with both knees off, bit-exactly; the
`mid-at-base-offset` placement ships (`--anchor-mid-offset`); the calibration gain is
`density.scale`. The work here is **defaults and wiring, not new arithmetic** — which
is also the strongest acceptance test available (see below).

Three consequences worth stating up front:

- **`Dmax` leaves the path.** `mid-at-base-offset` never reads the reference, so
  `--d-max`, `--auto-d-max` and `estimate --d-max-region` stop mattering for the
  decode; what that means for the flags is
  [the audit](../nf-core/knob-availability-audit.md)'s.
- **`density.scale` is one global value** — never per stock, roll or frame. What one
  value cannot reach is a rendering correction, not a second decode.
- **`offset` defaults to `[0, 0, 0]`** as a pick, not a closed question: the term is
  real, but no candidate has survived review and the data that could identify one does
  not exist yet.

## Open questions

- **Exponential or `generic-c41`?** The ADX precedent (a generic curve plus a matrix)
  argues for the averaged characteristic; against it, it inverts an averaged toe, and
  the two target different densitometries so a clean comparison needs a deferred
  calibration. A toe-limited form is the third candidate. Exponential is this task's
  default; changing it is a design change, not a tuning one.
- **Fresh module or the kept one?** The migration rule says write new stages fresh —
  but the decode is the one stage the redesign *keeps*, and its arithmetic is pinned by
  goldens. Decide explicitly.
- **Where the NC film RGB v1 3×3 belongs** — still at reconstruction's output, or
  moved into scene correction? That changes what `film-master` holds (Part 1, open).

## How to Verify

- Under `--new-flow` with no other flags, the pixels are **identical** to the old flow
  driven by the equivalent explicit flags. That equality is the claim that this is
  wiring; if it fails, something new was introduced and must be named.
- `film-master` exports the decode unchanged, and nothing on the new-flow path
  resolves a reference density.
- The four CI gates pass.

## Dependencies

- [The new stage module tree](../nf-core/stage-skeleton.md)
