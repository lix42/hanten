# What form should highlight desaturation take?

Produced by `nf-look/desaturation-spike`, 2026-09-21, on `2026-09-18-Gold200`. Set,
scripts and raw measurements in `../temp/desat-spike/` (uncommitted — the frames are the
user's photographs).

## The question, and how it changed

The design says chroma goes to zero as a pixel approaches diffuse white, without saying
*measured on what*. Two families were plausible: a **per-channel curve**, which is what
film, paper and all three outside converters do, and a **chroma pull** toward the neutral
axis. The spike expected them to look different.

They do not. What matters is a third thing neither of them had.

## Method

Eight frames, chosen by measurement rather than by eye: highlight chroma above L\* 90,
median across the three outside producers, split into **saturated** (1820, 1799, 1793,
1796 — C\* 11.8–15.2) and **neutral** (1789, 1811, 1810 — C\* 1.8–2.2), plus 1774 as the
reference frame.

White is pinned to **each frame's own** p97 throughout. Under the base-referenced anchor
nothing reaches the operator's range at all (`white-placement.md`), and a per-frame
anchor is the wrong rule but the right control here: the question is the operator's
form, so the anchor is held where both families can act.

The per-channel family needs no new code — the sigmoid's shoulder already is one. The
chroma pull was a throwaway patch in `render_split::display_source`, driven by an
environment variable rather than a flag, and reverted afterwards.

## The families are perceptually equivalent

Matched on **both** chroma removal and brightness — which took `--print-exposure 0.38`
and two attempts, because the exposure lands before the fit range and reinhard
re-compresses the lift:

| | L\* top | C\* top | hue shift |
|---|---|---|---|
| control | 80.6 | 17.0 | — |
| per-channel, matched | 80.4 | 9.5 | 5.1° |
| chroma pull | 80.6 | 8.8 | 4.3° |

**ΔL\* 0.16, ΔC\* 0.66, Δhue 0.7° — a total ΔE around 0.7, below visibility.**

The user reported this before the measurement caught up: *"I can tell the bright change,
but I cannot tell the color change."* That was a flaw in the set as well as a finding.
The arms had been matched on chroma removal and left unmatched on **brightness**, because
the per-channel curve compresses luminance as well as converging chroma (L\* 80.6 → 76.6)
while the pull holds it exactly.

## What actually separates them: one marked patch

Averages over the top 3% said "indistinguishable". Measuring **12 patches the user marked
by hand**, individually, said otherwise — on exactly one of them:

| 1820 "sand beach" (L\* 86.9) | C\* |
|---|---|
| control | 27.5 |
| per-channel curve | 7.6 |
| chroma pull | **1.1** |

Eleven of twelve patches agreed within C\* 1.4. The twelfth was the brightest and most
saturated, and the pull neutralised it almost completely — **because its strength keyed on
brightness alone, so a bright *coloured* surface was hit as hard as a bright *white* one.**

That is the sunset objection, reproduced on a real surface. No aggregate found it,
because such surfaces are rare enough to vanish in one; one marked patch was worth more
than eight frames of averaging.

## The fix: key on saturation as well

A band over linear-RGB saturation `(max − min)/max` multiplying the brightness term —
full pull below `s0`, off above `s1`, linear between. The patches place it: the
whites-with-cast run **0.137–0.279** and the sand beach sits at **0.463**, so a band and
not a single knee is what separates them (a knee low enough to protect the sand would
also have protected the "wall, should be close to white" at 0.253).

At 0.30 → 0.45:

| | whites with cast | genuinely coloured |
|---|---|---|
| control | 12.9 | 27.5 |
| per-channel, matched | 5.7 | 7.6 |
| chroma pull, unguarded | **6.1** | **1.1** |
| **chroma pull + guard** | **6.1** | **22.3** |

Identical cleanup, every other patch unchanged to within 0.1, and the sand beach keeps
22.3 against 1.1. Confirmed by eye: *"I can see the diff between 3 and 4. 4 keeps the
color."*

## The operator's reach is its threshold

How far a patch moves scales with how far above the start it sits — 96% at L\* 86.9, 44%
at 80.5, 25% at 74.9, and **0% at 67.9 and 63.9**. Two marked surfaces ("fog", one
"cloud") are untouched at every setting tried.

That is the boundary of what a *highlight* operator can do, not a defect; the user
confirmed those read fine, consistent with the eye being less sensitive to cast in shade.
Cast at L\* 64 belongs to the per-channel grade or to the decode's `scale`. It also means
**judging "are the whites clean" on a frame whose white sits at L\* 64 measures the
decode, not this stage.**

## Conclusions

1. **A chroma pull, not a per-channel curve** — for **separability**, not appearance. The
   curve moves luminance as well as chroma, and the fit range downstream eats whatever
   exposure compensates it, so the two jobs cannot be tuned apart.
2. **Strength keys on distance from the neutral axis as well as on brightness.**
3. **It is a highlight operator and cannot be more.**
4. **"Off" is wanted for less than the design assumed** — the guard answers the
   flatten-a-sunset objection, not the hide-a-cast one.

## What this does not settle

- **The parameters.** The band was fitted to **one** saturated patch on one roll. The
  shape carries; 0.30 and 0.45 do not. More marked saturated patches, on more rolls, is
  the cheapest way to advance it.
- **The measure.** Linear-RGB saturation was the cheap choice; a perceptual one may
  separate the cases better — the gap between 0.279 and 0.463 is real but not wide.
- **Hue exactness.** The pull's lerp toward `(Y, Y, Y)` holds luminance to 3e-3 but
  rotates hue 4.3°, because a straight line to the achromatic point in linear ACEScg is
  not a constant-hue path in CIELAB. Nothing suggested that rotation was visible.
- **The gamut map's share.** Nothing reachable by flag turns it off, so per-channel
  against control measures shoulder-plus-gamut-map jointly. `--display-tone shoulder`
  failed as a separator: it drives top-end chroma to exactly 0.0 with 17–29° of rotation,
  which is flattening rather than shaping — independent support for the design retiring
  it. **Since settled** (`docs/reports/gamut-map-share.md`, 2026-09-23): under this
  spike's reinhard control the map moved no marked patch. Its per-channel arms (reinhard
  plus a print exposure, which can lift pixels past display white where the map greys
  them) were not re-measured, so for those the share is still open.
