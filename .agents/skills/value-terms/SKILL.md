---
name: value-terms
description: >-
  Hanten's value vocabulary — which word names which number. Use before writing or
  reviewing any doc comment, help text, error or report message, recipe key or doc
  that names a value: high/low/bright/dark, density, film base / Dmin, anchor, stops,
  gain, scale, offset, slope, contrast, gamma, exposure, lift, headroom, white,
  luminance, neutral, saturation, hue, cast. Also when naming a new knob or field.
---

# Value terms

[`docs/terminology.md`](../../../docs/terminology.md) is the source of truth. Read
the section for the word you are about to write; don't work from memory or from
nearby code, which may predate it.

The rules it enforces, in brief:

- **Name the space** a number lives in (transmission, density, film RGB, ACEScg,
  display-linear) and the reference and sign of any stops figure.
- **One word per meaning.** Qualify a word with several meanings: "tone slope",
  "HDR headroom", "highlight range", "ACEScg luminance", "roll white".
- **Lead with "film base"** in user-facing text; `Dmin` is an alias only.
- **"Colour", "grey"** in prose and messages; identifiers keep their spelling.
- **Public vs internal.** A recipe key, flag, report field or stage id is the
  scripting contract: never rename one in passing. One that breaks the rules is
  listed under "Known public misnomers"; prose names the right concept and quotes
  the public name once. Help and message wording may change to follow the doc —
  update the tests that match it and `docs/using-nc.md` in the same change.

When you add a term or a knob, add it to the doc in the same change, tagged public
or internal.
