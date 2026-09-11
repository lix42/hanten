#!/usr/bin/env python3
"""Generate the visual-review set for the five conversion presets (`nc convert --preset`).

  characteristic-generic   characteristic curve, no stock (the averaged generic C-41)
  characteristic-stock     characteristic curve, the roll's own published response
  characteristic-aim       characteristic-stock + the aim-matched red density scale
  sigmoid-knees            sigmoid with its toe/shoulder, rendered with NO display tone
  sigmoid-flat             sigmoid with neither knee, rendered with extended Reinhard

**Every preset is calibrated to one target, not to its own taste.** The target is scene
mid-grey (0.18) rendered 0.31 stop up — the brightness the 2026-09-09 round approved — and
each preset carries whatever `print_exposure` lands it there. `nc` owns those numbers now;
this script names only the five presets, so a recalibration cannot leave the review set
rendering the previous constants.

It used to state the expansion by hand — a `--print-exposure` per preset (0.31 to 0.61)
and a per-stock red density scale — because `--preset` did not exist yet, which made this
script the **acceptance test** for the mechanism: rendering through the preset had to
produce byte-identical files. It did, on three frames across three rolls (2026-09-10), and
the constants were deleted in the same change. The two facts they encoded are now pinned
in code, where they are checked on every run:

  * the brightness calibration, by
    `pipeline::stages::midtone_placement::every_preset_lands_the_shared_brightness_target`;
  * the aim-matched scale — a **reciprocal**, since `--density-scale` multiplies the
    *scan's* density where the aim factor scales the *table's* — by
    `algo::film_stock::aim_red_scale` and its tests.

A roll is no longer skipped up front when its stock has no usable aim delta (the old
`AIM_RED` table was the gate). `nc` now refuses only the `chr-aim` rendition, so such a
roll yields four good cells and one reported failure rather than vanishing from the page
entirely — more useful, but it does mean a missing column is worth reading as "that stock's
sheet states no usable delta", not as a bug.

Writes to a throwaway directory OUTSIDE the repo — the frames are the user's own
photographs and are never committed. Only this script is.

Env: NC_PRESET_FRAMES (default all ten), NC_PRESET_OUT (default ../temp/preset-review).
"""
import json, os, pathlib, subprocess, sys

REPO = pathlib.Path(__file__).resolve().parents[2]
NC = REPO / "target/release/nc"
ASSETS = (REPO / "../nc-assets").resolve()
OUT = pathlib.Path(os.environ.get("NC_PRESET_OUT", REPO / "../temp/preset-review")).resolve()

# Roll -> the `--film-stock` name whose sheet the registry carries for it.
STOCK = {
    "2026-07-24-Gold200": "gold-200",
    "Ektar": "ektar-100",
    "Portra160-2026-07-22": "portra-160",
}

# (id, button label, `--preset` name, needs --film-stock, tooltip).
#
# The stock column is **stated per preset, never derived from the name.** `nc` refuses
# `--film-stock` beside a preset with no stock to configure and requires it for the two
# that have one, so guessing from the spelling ("stock" or "aim" in the name) would be a
# second copy of `ConversionPreset::needs_film_stock` — which the Rust side made an
# exhaustive match precisely so a new preset states its answer. A future name that broke
# the guess would fail here as an exit-2 rendition, reported and skipped, so the page
# would quietly lose a column rather than say anything.
PRESETS = [
    ("chr-generic", "chr generic", "characteristic-generic", False,
     "characteristic curve, no --film-stock: the average of nine published sheets"),
    ("chr-stock", "chr stock", "characteristic-stock", True,
     "characteristic curve, the roll's own published response"),
    ("chr-aim", "chr aim", "characteristic-aim", True,
     "characteristic-stock plus the aim-matched red density scale"),
    ("sig-knees", "sig knees + linear", "sigmoid-knees", False,
     "sigmoid with its toe/shoulder, no display tone curve; brightness from the anchor"),
    ("sig-flat", "sig flat + reinhard", "sigmoid-flat", False,
     "sigmoid with neither knee, character carried by extended Reinhard"),
]


def main():
    if not NC.exists():
        sys.exit(f"build the release binary first: cargo build --release ({NC})")
    if not (ASSETS / "manifest.json").exists():
        sys.exit(f"no assets at {ASSETS}")
    fx = json.loads((REPO / "scripts/sigmoid-baseline/fixtures.json").read_text())
    keys = os.environ.get("NC_PRESET_FRAMES", ",".join(fx["frames"])).split(",")
    OUT.mkdir(parents=True, exist_ok=True)

    images, failures = [], []
    for key in keys:
        f = fx["frames"].get(key)
        if not f:
            print(f"{key}: not in fixtures.json, skipped")
            continue
        roll = f["roll"]
        src = ASSETS / "rolls" / roll / f["file"]
        if not src.exists():
            print(f"{key}: {src} missing, skipped")
            continue
        dmin = ",".join(str(c) for c in fx["rolls"][roll]["dmin"])
        # Skipped like every other missing input in this loop, rather than raising: a frame
        # from a roll whose sheet the registry does not carry is a gap in the tables, not a
        # bug in the script, and the other frames still make a reviewable set.
        stock = STOCK.get(roll)
        if stock is None:
            print(f"{key}: no --film-stock mapping for roll {roll}, skipped")
            continue
        renditions, casts = {}, {}
        for pid, _, preset, needs_stock, _ in PRESETS:
            dest = OUT / f"{key}-{pid}.jpg"
            # No `--d-max`: the sigmoid presets take the shipped fixed reference, which is
            # what a bare `--preset` resolves. Stating the roll's measured one here would
            # review a config the preset cannot reproduce.
            cmd = [
                str(NC), "convert", str(src),
                "--output-preset", "gain-map-hdr",
                "--film-base", dmin,
                "--preset", preset,
                *(["--film-stock", stock] if needs_stock else []),
                "-o", str(dest), "--report", "json",
            ]
            r = subprocess.run(cmd, capture_output=True, text=True)
            if r.returncode:
                msg = r.stderr.strip().splitlines()[-1][:160] if r.stderr.strip() else "?"
                print(f"{key}/{pid}: nc exited {r.returncode}: {msg}")
                failures.append(f"{key}/{pid}")
                continue
            renditions[pid] = dest.name
            m = json.loads(r.stdout).get("output_stats", {}).get("mean")
            if m and m[0] > 0:
                casts[pid] = (m[1] / m[0], m[2] / m[0])
            print(f"{key:4} {pid:12} -> {dest.name}")
        if not renditions:
            continue
        images.append({
            "id": key,
            "label": f"{key} — {roll} · {f['file']}",
            "note": " · ".join(
                f"{pid} G/R {g:.3f} B/R {b:.3f}" for pid, (g, b) in casts.items()
            ),
            "renditions": renditions,
        })

    if not images:
        sys.exit(f"no frames rendered from {keys!r} — nothing to review")

    review = {
        "schema_version": 1,
        "title": "The five conversion presets",
        "description": (
            "Each button is one `--preset`. All five are calibrated to the same target "
            "— scene mid-grey 0.18 "
            "delivered at 0.223, the brightness approved on 2026-09-09 — so brightness is "
            "held constant and what differs is the reconstruction and the display tone. "
            "chr-generic is the proposed default. Two of them change the display stage as "
            "well as the curve: sig-knees applies no display tone at all (the sigmoid's own "
            "shoulder does that work), while the other four use extended Reinhard at 6 "
            "stops. Each button is `nc convert --preset <name>` and nothing else — the "
            "exposure and the density scale come from the preset."
        ),
        "configs": [
            {"id": pid, "label": label, "note": f"{note} — --preset {preset}"}
            for pid, label, preset, _, note in PRESETS
        ],
        "images": images,
    }
    (OUT / "review.json").write_text(json.dumps(review, indent=2) + "\n")
    print(f"\n{len(images)} frames x {len(PRESETS)} presets -> {OUT}/review.json")
    if failures:
        print(f"FAILED renditions ({len(failures)}): {', '.join(failures)}")
    print(f"\n  cd tools/review-app && pnpm dev {OUT}/review.json")


if __name__ == "__main__":
    main()
