"""Tests for nctool.cast: the chart-cast grid, patch sampling and summaries."""
from __future__ import annotations

import json
import os
import tempfile
import unittest

from nctool import cast, charts
from nctool.test_charts import CLASSIC, CORNERS

try:
    import numpy as np
except ImportError:  # pragma: no cover - CI sets NCTOOL_REQUIRE_DEPS
    if os.environ.get("NCTOOL_REQUIRE_DEPS"):
        raise
    np = None


def painted(target: dict, corners, values: dict, shape=(600, 800)):
    """A linear image with each listed patch flat at its RGB, the rest at 0.05."""
    rows, cols = shape
    w, h = target["size"]
    inv = charts.homography([[x * cols, y * rows] for x, y in corners],
                            [[0, 0], [w, 0], [w, h], [0, h]])
    yy, xx = np.mgrid[0:rows, 0:cols] + 0.5
    den = inv[2][0] * xx + inv[2][1] * yy + inv[2][2]
    u = (inv[0][0] * xx + inv[0][1] * yy + inv[0][2]) / den
    v = (inv[1][0] * xx + inv[1][1] * yy + inv[1][2]) / den
    img = np.full((rows, cols, 3), 0.05)
    for p in target["patches"]:
        if p["id"] in values:
            x, y, pw, ph = p["rect"]
            img[(u >= x) & (u < x + pw) & (v >= y) & (v < y + ph)] = values[p["id"]]
    return img


def p3_to_xyz_d50():
    from nctool import metrics
    m = metrics._mat_mul(metrics.bradford_adaptation("d65", "d50"),
                         metrics.rgb_to_xyz_d65(metrics.SPACES["display-p3"]))
    return np.asarray(m, dtype=np.float64)


class TestGrid(unittest.TestCase):
    def test_the_default_grid(self):
        configs = cast.expand_grid(cast.DEFAULT_GRID)
        # direct + 3 placements x 2 desaturations x (balance off + 2 forms x 4 lines)
        self.assertEqual(len(configs), 1 + 3 * 2 * (1 + 2 * 4))
        self.assertIsNone(configs["direct"]["recipe"])
        c = configs["hybrid.hd0.8.tilt.mn-red"]
        self.assertEqual(c["recipe"], "tilt")
        self.assertEqual(c["args"], ["--midtone-neutral", "red", "--placement", "hybrid",
                                     "--highlight-desaturation", "0.8"])
        self.assertIn("--neutral-balance", configs["frame.hd0.wb-off"]["args"])

    def test_extra_configurations_join_and_may_not_repeat_a_name(self):
        grid = {**cast.DEFAULT_GRID, "placement": ["hybrid"], "highlight_desaturation": ["0.8"],
                "extra": {"offset": {"recipe": "gain", "args": ["--density-offset", "0,0,0.04"]}}}
        self.assertEqual(cast.expand_grid(grid)["offset"]["args"][0], "--density-offset")
        grid["extra"] = {"direct": {"args": []}}
        with self.assertRaisesRegex(ValueError, "repeats"):
            cast.expand_grid(grid)

    def test_graft_takes_the_chart_frames_tones_and_leaves_the_input_alone(self):
        mine = {"params": {"roll": {"frames": {"1.tif": {"tones": "pic"}}}}}
        both = {"params": {"roll": {"frames": {"1.tif": {"tones": "other"},
                                                "9.tif": {"tones": "chart"}}}}}
        out = cast.graft_tones(mine, both, ["9.tif"])
        self.assertEqual(out["params"]["roll"]["frames"],
                         {"1.tif": {"tones": "pic"}, "9.tif": {"tones": "chart"}})
        self.assertNotIn("9.tif", mine["params"]["roll"]["frames"])
        with self.assertRaisesRegex(ValueError, "no entry for 8.tif"):
            cast.graft_tones(mine, both, ["8.tif"])

    def test_roll_frames_groups_by_role(self):
        m = {"rolls": {"R": {"frames": [{"file": "rolls/R/2.tif", "role": "chart"},
                                        {"file": "rolls/R/1.tif"},
                                        {"file": "rolls/R/base.tif", "role": "unexposed"}]}}}
        frames = cast.roll_frames(m, "R")
        self.assertEqual([cast.stem(f) for f in frames["chart"]], ["2"])
        self.assertEqual([cast.stem(f) for f in frames["real"]], ["1"])
        with self.assertRaisesRegex(ValueError, "no roll"):
            cast.roll_frames(m, "S")

    def test_render_commands_pin_the_destination_and_need_a_base_without_a_recipe(self):
        with tempfile.TemporaryDirectory() as work:
            recipe = os.path.join(work, "r.json")
            with open(recipe, "w") as f:
                json.dump({"params": {"calibration": {"film_base": {"explicit": [0.5, 0.2, 0.1]}}}}, f)
            frames = {"chart": [{"file": "rolls/R/7.tif"}]}
            configs = {"direct": {"recipe": None, "args": ["--rendering", "direct"]},
                       "g": {"recipe": "gain", "args": []}}
            cmds = cast.render_commands("hanten", "/A", frames, work, configs, {"gain": recipe})
            direct, g = (c for c, _ in cmds)
            self.assertIn("0.5,0.2,0.1", direct)
            self.assertEqual(g[g.index("--params") + 1], recipe)
            for c in (direct, g):
                self.assertEqual(c[c.index("--gamut") + 1], "display-p3")
            with self.assertRaisesRegex(ValueError, "no measured film base"):
                cast.render_commands("hanten", "/A", frames, work, configs, {})


@unittest.skipIf(np is None, "numpy not installed")
class TestMeasure(unittest.TestCase):
    def setUp(self):
        self.target = charts.load_target(CLASSIC)
        self.entries = [{"target": CLASSIC, "corners": CORNERS}]

    def rows(self, tint=(1.0, 1.0, 1.0), frame="1"):
        greys = {pid: [0.03 + 0.15 * i * t for t in tint]
                 for i, pid in enumerate(reversed(self.target["neutrals"]))}
        img = painted(self.target, CORNERS, greys)
        return cast.patch_rows(img, p3_to_xyz_d50(), frame, self.entries)

    def test_equal_rgb_greys_read_neutral_after_the_d50_adaptation(self):
        rows = [r for r in self.rows() if r["role"] == "neutral"]
        self.assertEqual(len(rows), 6)
        for r in rows:
            self.assertLess(abs(r["lab"][1]) + abs(r["lab"][2]), 0.05, r)

    def test_a_blue_tint_reads_negative_b(self):
        s = cast.summarize(self.rows(tint=(1.0, 1.0, 1.3)))
        self.assertLess(s["all"]["b"], -3)
        self.assertGreater(s["all"]["c"], 3)

    def test_the_matched_frame_is_the_one_whose_mid_grey_lands_nearest_the_reference(self):
        dark = self.rows(frame="dark")
        light = [dict(r, frame="light", lab=[r["lab"][0] + 30, *r["lab"][1:]]) for r in dark]
        m = cast.summarize(dark + light)["matched"]
        ref_l = self.target["reference"]["values"]["D4"][0]
        self.assertEqual(m["patch"], "D4")
        rendered = {f: next(r["lab"][0] for r in rows if r["patch"] == "D4")
                    for f, rows in (("dark", dark), ("light", light))}
        self.assertEqual(m["frame"], min(rendered, key=lambda f: abs(rendered[f] - ref_l)))

    def test_colour_accuracy_is_zero_at_the_reference(self):
        rows = [{"frame": "1", "role": "colour", "patch": "A", "lab": [50, 40, 20],
                 "ref": [50, 40, 20]},
                {"frame": "1", "role": "colour", "patch": "B", "lab": [50, 10, 10],
                 "ref": [50, 10, 10]}]
        acc = cast.colour_accuracy(rows)
        self.assertAlmostEqual(acc["hue_error"], 0.0)
        self.assertAlmostEqual(acc["chroma_ratio"], 1.0)
        self.assertAlmostEqual(acc["delta_e"], 0.0)


class TestDensity(unittest.TestCase):
    def test_a_constant_blue_offset_is_recovered_with_unit_slopes(self):
        scale = [1.0, 0.8, 0.5]
        patches = [{"role": "neutral", "density": [g, g / scale[1], (g - 0.04) / scale[2]]}
                   for g in (0.25, 0.45, 0.65, 0.85, 1.2)]
        frames = {"1": {"charts": [{"patches": patches + [{"role": "colour",
                                                           "density": [9, 9, 9]}]}]}}
        d = cast.density_residuals(frames, scale)
        self.assertAlmostEqual(d["offset"]["blue"], 0.04)
        self.assertAlmostEqual(d["offset"]["red"], 0.0)
        self.assertEqual(d["offset"]["n"], 4)   # 1.2 lies past DENSITY_RANGE
        self.assertAlmostEqual(d["slope"]["red"], 1.0)
        self.assertAlmostEqual(d["slope"]["blue"], 1.0)

    def test_no_greys_gives_nothing(self):
        self.assertEqual(cast.density_residuals({}, [1, 1, 1]), {})


if __name__ == "__main__":
    unittest.main()
