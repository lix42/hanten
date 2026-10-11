# Black-and-white in the chain

Where a monochrome negative becomes mono, what the NC film RGB v1 3×3 means there, and
whether silver and chromogenic B&W differ. Decided by `nf-reconstruction/mono-decode`
(2026-10-09). Parts that wait on real scans are marked **pending**. No HP5 scans were
in the asset set when this was written.

## The rule: the film's signal count decides, not the file's

A silver negative carries **one** image signal. The scanner may record it as one gray
channel or as three near-equal RGB channels, and both recordings must render alike. A
colour negative carries three dye signals, even when the photograph is meant to be mono.
A chromogenic B&W negative carries one signal recorded in dye; where it belongs is
pending (below).

| Scan | Film | Where it becomes mono |
|---|---|---|
| one gray channel | any | **the decode** (mono input) |
| three channels | silver, declared (`--film-type silver`) | **the decode** (mono input), pooled |
| three channels | colour rendered mono; chromogenic B&W for now | **the look** (a rendering choice) |
| three channels | silver, not declared | the colour route (see "Traps") |

So mono is **neither a whole-chain mode nor one stage**. It enters at one of two
points, and after it enters no stage needs its own mono branch, because every stage
downstream already keeps a neutral pixel neutral.

## Mono input: the decode

**One channel.** Decode it with one curve and carry it as R = G = B into the 3×3
(whether the reader widens it or film base handles one channel is `mono-input`'s). The film master is then R = G = B up to the 3×3's rounding (below), which is
honest: the scan held one signal.

**Three channels, declared silver.** Divide each channel by its own film base first,
which removes the base's slight tint and any per-channel scanner gain. Then pool the
three channels into one signal and continue exactly as for one channel. After this
step a three-channel silver scan is indistinguishable from a one-channel one.

- **The switch is `input.film_type`** (`--film-type silver`), the declared chemistry
  axis that was kept for this purpose. The decode **is** the right place to act on a
  declaration. The IR holder detection, by contrast, must not use it, because there a
  measurement beats the declaration (design-spec §6.1); that rule is unchanged.
- **Pooling weights and domain are pending.** Whether to pool transmission or density,
  and with which weights, should match what the scanner does when it writes a
  one-channel file. Measure that by comparing the two recordings of the same frame.

**One curve.** `scale [1, 0.84, 0.73]` is a fit of the colour chain, dye layers and
scanner channels together (`algo/fixed.rs`). Applied to one silver signal copied into
three, it would give each channel its own curve: a tint that changes with brightness,
which white balance cannot remove. The mono decode reads only the red entries
(`scale_r = 1` is the convention). Whether the scanner's share of `scale` matters
before pooling a three-channel silver scan, whether a *stated* green or blue entry that
differs from red is refused, and whether silver gets its own default are pending
(below). Chromogenic B&W runs the colour route, so it gets the colour `scale` until
measured.

**The 3×3 is identity on the neutral axis**, up to rounding. Each NC film RGB v1 row
sums to 1 within about 5e-8 (`working_space`'s
`neutral_maps_to_neutral_rows_sum_to_one`), so R = G = B leaves it with channels
within a couple of f32 ULP of each other. The declaration that film channels are
Rec.709 primaries is meaningless for one signal, and harmless.

**Knobs that can only tint are refused** on mono input, with a message saying why:
a stated white balance, the midtone neutral and the look's `channel_grade`. Identity
values a recipe can carry (white balance `[1, 1, 1]`, `channel_grade` `[1, 1]`) are
spared, and the midtone neutral, on by default, is refused only when active; the exact
rule is `mono-input`'s. A roll measurement over mono input writes no white-balance
gains and no midtone line, and measures everything else as for a colour roll, so
`hanten roll` keeps working. Toning (sepia, split-tone) is a separate future feature,
not something these knobs should produce by accident. Everything else applies
unchanged: exposure, the look's contrast (it works on luminance), fit range (it
scales all three channels together), and fit gamut. Saturation and highlight
desaturation do nothing to a neutral pixel.

## Mono as a rendering: the look

A three-channel scan of a dye negative rendered mono is pooled **first in the look**,
after scene correction, with weights the user can set (a channel mixer, NLP's
"Color Model: B+W"). Two consequences:

- **White balance still matters.** It acts before pooling, so it changes the grey
  tones even though it can no longer tint. It is not the same thing as the weights,
  which act after scene correction.
- **Chromogenic B&W takes this route for now.** By the rule it is one signal, so the
  mono decode may suit it better, but `--film-type chromogenic` cannot tell it from
  colour film and no scan exists to measure its dye. Its IR plane is usable for dust
  removal either way.

A zero `look.saturation` (`--saturation`) would already render each pixel as its
luminance, but the value rule refuses 0, and a fixed luminance has no weights. The weighted pool is
its own control.

## Output

Three channels with R = G = B, through the unchanged destination and ICC path. The
equality holds to within f32 rounding through the 3×3 and lcms2, not bit-exactly. A
true grayscale encode (one channel, a gray ICC profile), or an exact pool just before
the encode, is a follow-up.

## Traps

- **A three-channel silver scan with no declaration takes the colour route**, where
  the colour `scale` gives it a tint that changes with brightness. Nothing infers the
  film type (the project uses no automatic classification), so the user guide must
  say so. A report warning keyed on measured channel spread is possible later.
- **Silver blocks IR in proportion to density**, so on silver the IR plane is no use
  for dust removal. The holder detection is gated on the measured
  separability (design-spec §6.1), so a dense silver frame falls back to the static
  inset rather than masking wrongly. The future dust-removal stage must be disabled for silver, and
  `input.film_type` is the declaration that can do it.
- **Memory.** A one-channel scan copied into three f32 channels has its own
  bytes-per-pixel figures, which `pipeline::memory` must model rather than inherit.

## Pending, with the HP5 scans (one-channel and three-channel recordings)

- How close the three channels of a silver scan are, and how each channel's base
  differs from the others.
- Whether the scanner's one-channel file is one of its RGB channels or a mix of them.
  That sets the default pooling weights and the domain.
- Whether `scale` should be `[1, 1, 1]` for silver (its own default), whether a stated
  green or blue entry that differs from red is refused, and whether the copied
  one-channel decode reproduces the pooled three-channel one.
- IR on silver: the measured `ir_separability` on an exposed frame.

## Pending, with a chromogenic B&W scan

- Whether chromogenic B&W takes the mono decode, and how it would be declared.
