#!/usr/bin/env python3
"""Generate the visual-review set for the five conversion presets (algo/film-stock-profiles).

`--preset` does not exist yet. This renders each preset's **expansion** with plain flags,
so the pixels are what the preset will produce once the mechanism ships — which makes this
set the acceptance test for it: regenerating through `--preset` must give identical files.

  characteristic-generic   characteristic curve, no stock (the averaged generic C-41)
  characteristic-stock     characteristic curve, the roll's own published response
  characteristic-aim       characteristic-stock + the aim-matched red density scale
  sigmoid-knees            sigmoid with its toe/shoulder, rendered with NO display tone
  sigmoid-flat             sigmoid with neither knee, rendered with extended Reinhard

**Every preset is calibrated to one target, not to its own taste.** The target is scene
mid-grey (0.18) rendered 0.31 stop up — the brightness the 2026-09-09 round approved — and
each preset's `--print-exposure` is whatever lands it there. The four values that use it
differ (0.31 to 0.61) because the reconstructions place mid-grey differently: the
characteristic curve reads it off the film, while the sigmoid's anchor puts it ~0.33 stop
lower. The fifth, `sigmoid-knees`, has to take its brightness from the anchor instead and so
states 0.0 — see the note on its row below. Measured by
`pipeline::stages::midtone_placement::each_candidate_look_needs_its_own_print_exposure`,
which fails if the spread ever collapses to the point one shared default would do.

Two conventions that are easy to get backwards, both measured rather than derived:

*The aim-matched scale is a reciprocal.* `curve_probe::stock_table_variants` prints the
factor scaling the **table's** red density (Ektar 0.898); `--density-scale` multiplies the
**scan's** density before the table is inverted, so the flag takes `1/k`. On 21 frames the
reciprocal takes the green-magenta drift to +0.01 stop/density; the table-side number takes
it to +0.72, worse than doing nothing (+0.35). Once `--preset` ships this is derived from
the shipped aim tables and the constants below disappear.

*`--print-exposure` is not comparable across tone operators.* These numbers are for
`extended-reinhard-mid-preserving-v2`, which absorbs its own 0.237-stop midtone cost; under
the v1 operator the same look was spelled 0.55.

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
# The aim-matched red scale as `--density-scale` takes it (the reciprocal; see above).
AIM_RED = {"ektar-100": 1.114, "portra-160": 1.029, "gold-200": 0.955}

# (id, button label, flags beyond the shared ones, print-exposure, tooltip)
PRESETS = [
    ("chr-generic", "chr generic", ["--density-curve", "characteristic"], 0.39,
     "characteristic curve, no --film-stock: the average of nine published sheets"),
    ("chr-stock", "chr stock", ["--density-curve", "characteristic", "STOCK"], 0.31,
     "characteristic curve, the roll's own published response"),
    ("chr-aim", "chr aim", ["--density-curve", "characteristic", "STOCK", "AIM"], 0.31,
     "characteristic-stock plus the aim-matched red density scale"),
    # **This one's brightness is in the anchor, not `--print-exposure`, and that is
    # forced.** `--display-tone none` relies on the reconstruction being bounded at the
    # render's ceiling; `--print-exposure` is a scalar gain applied after the curve, so any
    # positive value pushes the shoulder past reference white and the range check refuses
    # the frame (measured: +0.70 gave "luminance 1.6236" = exactly 2^0.70). Moving the
    # anchor instead places mid-grey *within* the bounded range. Fraction 0.42 lands the
    # shared target to 0.027 stop —
    # `midtone_placement::the_linear_rendered_sigmoid_takes_its_brightness_from_the_anchor`
    # sweeps it and fails if the calibration drifts.
    ("sig-knees", "sig knees + linear",
     ["--display-tone", "none", "--anchor-mid-fraction", "0.42"], 0.0,
     "sigmoid with its toe/shoulder, no display tone curve; brightness from the anchor"),
    ("sig-flat", "sig flat + reinhard", ["--sigmoid-toe", "0", "--sigmoid-shoulder", "0"], 0.61,
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
        if stock not in AIM_RED:
            print(f"{key}: no aim-matched red scale for {stock}, skipped")
            continue
        renditions, casts = {}, {}
        for pid, _, extra, exposure, _ in PRESETS:
            dest = OUT / f"{key}-{pid}.jpg"
            flags = []
            for token in extra:
                if token == "STOCK":
                    flags += ["--film-stock", stock]
                elif token == "AIM":
                    flags += ["--density-scale", f"{AIM_RED[stock]},1,1"]
                else:
                    flags.append(token)
            # `reinhard` unless the preset names its own tone, and no `--d-max`: the
            # sigmoid presets take the shipped fixed reference, which is what a bare
            # `--preset` will resolve. Stating the roll's measured one here would review a
            # config the preset cannot reproduce.
            if "--display-tone" not in flags:
                flags += ["--display-tone", "reinhard"]
            cmd = [
                str(NC), "convert", str(src),
                "--output-preset", "gain-map-hdr",
                "--film-base", dmin,
                "--print-exposure", str(exposure),
                *flags,
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
            "Each button is one proposed `--preset`, rendered through the flags it will "
            "expand to. All five are calibrated to the same target — scene mid-grey 0.18 "
            "delivered at 0.223, the brightness approved on 2026-09-09 — so brightness is "
            "held constant and what differs is the reconstruction and the display tone. "
            "chr-generic is the proposed default. Two of them change the display stage as "
            "well as the curve: sig-knees applies no display tone at all (the sigmoid's own "
            "shoulder does that work), while the other four use extended Reinhard at 6 "
            "stops."
        ),
        "configs": [
            {"id": pid, "label": label, "note": f"{note} — --print-exposure {exposure}"}
            for pid, label, _, exposure, note in PRESETS
        ],
        "images": images,
    }
    (OUT / "review.json").write_text(json.dumps(review, indent=2) + "\n")
    print(f"\n{len(images)} frames x {len(PRESETS)} presets -> {OUT}/review.json")
    if failures:
        print(f"FAILED renditions ({len(failures)}): {', '.join(failures)}")
    print(f"\n  http://localhost:8080/review-app/?data=../{OUT.name}/review.json")


if __name__ == "__main__":
    main()
