"""Tests for nctool.charts: geometry, the label schema, the placement check on a
synthetic negative, and `charts import` end to end."""
from __future__ import annotations

import argparse
import contextlib
import io
import json
import os
import tempfile
import unittest

from nctool import charts, manifest

try:
    import numpy as np
    import tifffile
except ImportError:  # pragma: no cover - CI sets NCTOOL_REQUIRE_DEPS
    if os.environ.get("NCTOOL_REQUIRE_DEPS"):
        raise
    np = tifffile = None

CLASSIC = "ccpp2-classic"
CORNERS = [[0.2, 0.15], [0.8, 0.2], [0.78, 0.85], [0.18, 0.8]]


class TestGeometry(unittest.TestCase):
    def test_homography_maps_the_corners_and_round_trips(self):
        t = charts.load_target(CLASSIC)
        h = charts.target_to_frame(t, CORNERS)
        w, hh = t["size"]
        for src, dst in zip([[0, 0], [w, 0], [w, hh], [0, hh]], CORNERS):
            for a, b in zip(charts.project(h, src), dst):
                self.assertAlmostEqual(a, b, places=9)
        back = charts.homography(CORNERS, [[0, 0], [w, 0], [w, hh], [0, hh]])
        mid = charts.project(back, charts.project(h, [20.0, 10.0]))
        self.assertAlmostEqual(mid[0], 20.0, places=9)
        self.assertAlmostEqual(mid[1], 10.0, places=9)

    def test_the_shared_case_matches_the_labelling_tool(self):
        # tools/review-app/src/chart.test.ts pins the same point.
        h = charts.target_to_frame(charts.load_target(CLASSIC), CORNERS)
        x, y = charts.project(h, [20.0, 10.0])
        self.assertAlmostEqual(x, 0.3344222625529341, places=12)
        self.assertAlmostEqual(y, 0.2755293405928615, places=12)

    def test_degenerate_corners_are_refused(self):
        with self.assertRaisesRegex(ValueError, "degenerate"):
            charts.homography([[0, 0], [1, 0], [2, 0], [0, 1]], CORNERS)

    def test_a_regular_grid_has_a_strip_in_every_gap_and_around_the_edge(self):
        strips = dict(charts.gap_strips(charts.load_target(CLASSIC)))
        self.assertEqual(len(strips), (5 * 4 + 6 * 3) + 2 * (6 + 4))
        x, _, w, _ = strips["A1|right"]
        self.assertTrue(12.0 < x and x + w < 15.0)  # inside the gap
        x, _, w, _ = strips["A1|left"]
        self.assertTrue(-3.0 < x and x + w < 0.0)  # outside the array, one gap out

    def test_the_creative_target_has_strips_between_its_irregular_rows(self):
        strips = dict(charts.gap_strips(charts.load_target("ccpp2-creative")))
        self.assertIn("M1a|right", strips)   # between the two middle columns
        self.assertIn("L1|below", strips)
        self.assertIn("N8|outer-below", strips)
        self.assertNotIn("M1a|top", strips)  # not on the array's edge

    def test_convexity_accepts_either_winding(self):
        self.assertTrue(charts.is_convex(CORNERS))
        self.assertTrue(charts.is_convex(CORNERS[::-1]))
        bow_tie = [CORNERS[0], CORNERS[2], CORNERS[1], CORNERS[3]]
        self.assertFalse(charts.is_convex(bow_tie))


class TestSchema(unittest.TestCase):
    LABEL = {"target": CLASSIC, "card": "ccpp2", "corners": CORNERS}

    def test_a_good_label_and_entry(self):
        self.assertEqual(charts.check_label(dict(self.LABEL)), [])
        entry = dict(self.LABEL, source="s", check={"pass": True})
        self.assertEqual(charts.check_entry(entry), [])

    def test_bad_labels(self):
        cases = {
            "target 'x' is not one of": dict(self.LABEL, target="x"),
            "card must be": dict(self.LABEL, card=""),
            "four [x, y]": dict(self.LABEL, corners=CORNERS[:3]),
            "inside the frame": dict(self.LABEL, corners=[[1.2, 0]] + CORNERS[1:]),
            "convex": dict(self.LABEL, corners=[CORNERS[0], CORNERS[2], CORNERS[1], CORNERS[3]]),
            "unknown key 'patches'": dict(self.LABEL, patches=[]),
        }
        for want, label in cases.items():
            self.assertTrue(any(want in e for e in charts.check_label(label)), want)

    def test_a_malformed_entry_is_reported_not_raised(self):
        ok = dict(self.LABEL, source="s", check={"pass": True})
        for bad in ({"card": "ccpp2"}, dict(ok, target=["x"]), dict(ok, card=None)):
            errs = charts.check_frame_charts({"charts": [ok, bad]})
            self.assertTrue(errs, bad)
        self.assertIn("oriented, when present, must be true",
                      charts.check_label(dict(self.LABEL, oriented="yes")))

    def test_an_entry_needs_a_passed_check_and_one_per_target_and_card(self):
        entry = dict(self.LABEL, source="s", check={"pass": False})
        self.assertIn("check must be a placement check that passed", charts.check_entry(entry))
        ok = dict(entry, check={"pass": True})
        errs = charts.check_frame_charts({"charts": [ok, dict(ok)]})
        self.assertEqual(errs, [f"chart {CLASSIC} on card 'ccpp2' appears twice"])
        self.assertEqual(charts.check_frame_charts({"charts": {}}),
                         ["charts is not a list (dict)"])


def synthetic_negative(target: dict, corners, shape=(600, 800), seed=0):
    """A negative of the target placed at `corners`: each patch flat (plus grain), the
    neutral series rising in transmission from white to black, gaps and surround at
    the black end."""
    rng = np.random.default_rng(seed)
    rows, cols = shape
    w, h = target["size"]
    inv = charts.homography([[x * cols, y * rows] for x, y in corners],
                            [[0, 0], [w, 0], [w, h], [0, h]])
    yy, xx = np.mgrid[0:rows, 0:cols] + 0.5
    den = inv[2][0] * xx + inv[2][1] * yy + inv[2][2]
    u = (inv[0][0] * xx + inv[0][1] * yy + inv[0][2]) / den
    v = (inv[1][0] * xx + inv[1][1] * yy + inv[1][2]) / den
    img = np.full((rows, cols, 3), 40000.0)
    neutrals = target["neutrals"]
    for p in target["patches"]:
        x, y, pw, ph = p["rect"]
        inside = (u >= x) & (u < x + pw) & (v >= y) & (v < y + ph)
        if p["id"] in neutrals:
            level = 2000.0 * 2 ** (0.8 * neutrals.index(p["id"]))
            img[inside] = [level] * 3
        else:  # a saturated colour: one channel a stop or more off the others
            img[inside] = rng.permutation([3000.0, 6000.0, 20000.0]) * rng.uniform(0.5, 1.5)
    img *= 2 ** rng.normal(0, 0.03, img.shape)
    return np.clip(img, 1, 65535).astype(np.uint16)


@unittest.skipIf(np is None, "numpy/tifffile not installed")
class TestPlacementCheck(unittest.TestCase):
    def setUp(self):
        self.t = charts.load_target(CLASSIC)
        self.scan = synthetic_negative(self.t, CORNERS).astype(float)

    def test_the_right_corners_pass(self):
        r = charts.check_placement(self.scan, self.t, CORNERS)
        self.assertTrue(r["pass"], r["failures"])
        self.assertTrue(all(s > 0 for s in r["neutral_steps"]))

    def test_a_few_bright_gaps_are_warnings_not_failures(self):
        # Blur or flare lifts a gap or two; the grid is still right.
        scan = self.scan.copy()
        h = charts.target_to_frame(self.t, CORNERS)
        rect = dict(charts.gap_strips(self.t))["A1|right"]
        quad = [charts.project(h, p) for p in charts.rect_quad(rect)]
        rows, cols = scan.shape[:2]
        yy, xx = charts._mask((rows, cols), [[x * cols, y * rows] for x, y in quad])
        scan[yy, xx] = 3000.0
        r = charts.check_placement(scan, self.t, CORNERS)
        self.assertTrue(r["pass"], r["failures"])
        self.assertEqual(r["bright_gaps"], 1)
        self.assertIn("gap A1|right", r["warnings"][0])

    def test_a_corner_off_by_half_a_patch_fails(self):
        bad = [list(c) for c in CORNERS]
        bad[2][0] += 0.05
        r = charts.check_placement(self.scan, self.t, bad)
        self.assertFalse(r["pass"])

    def test_a_grid_one_patch_off_fails(self):
        dx = 15.0 / self.t["size"][0] * (CORNERS[1][0] - CORNERS[0][0])
        moved = [[x + dx, y] for x, y in CORNERS]
        r = charts.check_placement(self.scan, self.t, moved)
        self.assertFalse(r["pass"])

    def test_any_turn_or_mirror_of_the_corners_is_put_back_in_order(self):
        for quad in charts.orientations(CORNERS):
            r = charts.find_placement(self.scan, self.t, quad)
            self.assertTrue(r["pass"], quad)
            self.assertEqual(r["corners"], CORNERS)
            self.assertEqual(r["reoriented"], quad != CORNERS)

    def test_several_passing_orientations_are_not_guessed(self):
        # A target whose neutrals read flat both ways (a synthetic card with no
        # neutral steps) passes upside down too: the user must say which.
        t = dict(self.t, neutrals=["A4"], saturated=[])
        r = charts.find_placement(self.scan, t, CORNERS)
        self.assertFalse(r["pass"])
        self.assertIn(CORNERS, r["candidates"])
        self.assertGreater(len(r["candidates"]), 1)
        self.assertIn("click the white patch (A4)", r["failures"][0])

    def test_an_oriented_label_checks_only_its_own_order(self):
        t = dict(self.t, neutrals=["A4"], saturated=[])
        r = charts.find_placement(self.scan, t, CORNERS, oriented=True)
        self.assertTrue(r["pass"], r["failures"])
        self.assertNotIn("candidates", r)
        upside_down = CORNERS[2:] + CORNERS[:2]
        self.assertFalse(charts.find_placement(self.scan, self.t, upside_down, True)["pass"])

    def test_a_patch_outside_the_scan_fails_and_a_strip_outside_is_skipped(self):
        off = [[x, y - 0.5] for x, y in CORNERS]
        r = charts.check_placement(self.scan, self.t, off)
        self.assertFalse(r["pass"])
        self.assertIn("lie outside the scan", r["failures"][0])
        json.dumps(r, allow_nan=False)  # nothing non-finite reaches the JSON
        # The array's top edge on the frame's: the outer "top" strips fall off it.
        flush = [[0.2, 0.0], [0.8, 0.0], [0.8, 0.6], [0.2, 0.6]]
        scan = synthetic_negative(self.t, flush).astype(float)
        r = charts.check_placement(scan, self.t, flush)
        self.assertTrue(r["pass"], r["failures"])
        self.assertEqual(r["bright_gaps"], 0)  # the off-frame strips were not counted

    def test_a_misplaced_label_fails_in_every_orientation(self):
        bad = [list(c) for c in CORNERS]
        bad[2][0] += 0.05
        r = charts.find_placement(self.scan, self.t, bad)
        self.assertFalse(r["pass"])
        self.assertEqual(r["corners"], bad)  # reported as given

    def test_a_target_rotated_half_a_turn_fails(self):
        r = charts.check_placement(self.scan, self.t, CORNERS[2:] + CORNERS[:2])
        self.assertFalse(r["pass"])


@unittest.skipIf(np is None, "numpy/tifffile not installed")
class TestImport(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.A = tmp.name
        t = charts.load_target(CLASSIC)
        os.makedirs(os.path.join(self.A, "rolls", "R"))
        tifffile.imwrite(os.path.join(self.A, "rolls", "R", "2127.tif"),
                         synthetic_negative(t, CORNERS))
        self.prior = {"schema_version": 4, "rolls": {"R": {"stock": "s", "frames": [
            {"file": "rolls/R/2127.tif", "role": "chart",
             "charts": [{"target": CLASSIC, "card": "other", "corners": CORNERS,
                         "source": "old", "check": {"pass": True}}]}]}}}
        with open(os.path.join(self.A, "manifest.json"), "w") as f:
            json.dump(self.prior, f)

    def labels(self, corners):
        path = os.path.join(self.A, "labels.json")
        with open(path, "w") as f:
            json.dump({"roll": "R", "frames": {"2127": [
                {"target": CLASSIC, "card": "ccpp2", "corners": corners}]}}, f)
        return path

    def run_import(self, corners):
        args = argparse.Namespace(asset_root=self.A, labels=self.labels(corners),
                                  source="test", date="2026-10-14", dry_run=False)
        err = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            rc = charts.cmd_import(args)
        with open(os.path.join(self.A, "manifest.json")) as f:
            return rc, json.load(f), err.getvalue()

    def test_a_passing_label_is_added_beside_other_cards(self):
        rc, m, err = self.run_import(CORNERS)
        self.assertEqual(rc, 0, err)
        cs = m["rolls"]["R"]["frames"][0]["charts"]
        self.assertEqual([c["card"] for c in cs], ["other", "ccpp2"])
        self.assertEqual(cs[1]["check"]["date"], "2026-10-14")
        self.assertTrue(cs[1]["check"]["pass"])
        self.assertEqual(m["schema_version"], manifest.SCHEMA)
        self.assertEqual(charts.check_frame_charts(m["rolls"]["R"]["frames"][0]), [])

    def test_corners_given_out_of_order_are_stored_in_the_targets_order(self):
        rc, m, err = self.run_import(CORNERS[2:] + CORNERS[:2])
        self.assertEqual(rc, 0, err)
        self.assertEqual(m["rolls"]["R"]["frames"][0]["charts"][1]["corners"], CORNERS)

    def test_a_cleared_or_re_carded_label_leaves_the_manifest(self):
        rc, m, err = self.run_import(CORNERS)
        self.assertEqual(rc, 0, err)
        # Re-carded in the tool: the same source's old entry goes.
        path = os.path.join(self.A, "labels.json")
        with open(path, "w") as f:
            json.dump({"roll": "R", "frames": {"2127": [
                {"target": CLASSIC, "card": "ccpp2-b", "corners": CORNERS}]}}, f)
        args = argparse.Namespace(asset_root=self.A, labels=path, source="test",
                                  date="2026-10-14", dry_run=False)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(charts.cmd_import(args), 0)
        with open(os.path.join(self.A, "manifest.json")) as f:
            cards = [c["card"] for c in json.load(f)["rolls"]["R"]["frames"][0]["charts"]]
        self.assertEqual(cards, ["other", "ccpp2-b"])  # another source's entry stays
        # Cleared in the tool: listed with no labels.
        with open(path, "w") as f:
            json.dump({"roll": "R", "frames": {"2127": []}}, f)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(charts.cmd_import(args), 0)
        with open(os.path.join(self.A, "manifest.json")) as f:
            cards = [c["card"] for c in json.load(f)["rolls"]["R"]["frames"][0]["charts"]]
        self.assertEqual(cards, ["other"])

    def test_a_failing_label_writes_nothing(self):
        bad = [list(c) for c in CORNERS]
        bad[2][0] += 0.05
        rc, m, err = self.run_import(bad)
        self.assertEqual(rc, 1)
        self.assertIn("placement check failed", err)
        self.assertEqual(m, self.prior)


if __name__ == "__main__":
    unittest.main()
