"""Tests for nctool.chart_table: the per-patch table read from labelled chart frames."""
from __future__ import annotations

import argparse
import contextlib
import io
import json
import os
import tempfile
import unittest
from unittest import mock

from nctool import chart_table, charts, manifest
from nctool.test_charts import CLASSIC, CORNERS, synthetic_negative

try:
    import numpy as np
    import tifffile
except ImportError:  # pragma: no cover - CI sets NCTOOL_REQUIRE_DEPS
    if os.environ.get("NCTOOL_REQUIRE_DEPS"):
        raise
    np = tifffile = None

BASE = [0.6, 0.3, 0.15]


def fake_base(path):
    return {"rgb": list(BASE), "hanten": {"nc_version": "test"}, "uniformity": "uniform",
            "warnings": []}


@unittest.skipIf(np is None, "numpy/tifffile not installed")
class TestTable(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.A = tmp.name
        os.makedirs(os.path.join(self.A, "rolls", "R"))
        t = charts.load_target(CLASSIC)
        entry = {"target": CLASSIC, "card": "ccpp2", "corners": CORNERS, "source": "s",
                 "check": {"pass": True, "rules": charts.CHECK_VERSION,
                           "target_version": t["version"]}}
        base = os.path.join(self.A, "rolls", "R", "base.tif")
        tifffile.imwrite(base, np.full((40, 40, 3), 30000, np.uint16))
        frames = [{"file": "rolls/R/base.tif", "role": "unexposed",
                   "sha256": manifest.sha256(base)}]
        # Two bracket frames: the second a stop brighter, so every patch denser.
        for stem, ev, gain in (("2127", -1, 1.0), ("2128", 0, 0.5)):
            img = (synthetic_negative(t, CORNERS).astype(float) * gain).astype(np.uint16)
            path = os.path.join(self.A, "rolls", "R", f"{stem}.tif")
            tifffile.imwrite(path, img)
            frames.append({"file": f"rolls/R/{stem}.tif", "role": "chart",
                           "sha256": manifest.sha256(path),
                           "shot": {"exposure_ev": ev, "bracket": "b"},
                           "charts": [json.loads(json.dumps(entry))]})
        self.manifest = {"schema_version": 4, "rolls": {"R": {
            "stock": "Kodak Gold 200", "conditions": {"light": "overcast"}, "frames": frames}}}

    def test_every_patch_of_every_chart_with_density_from_the_film_base(self):
        table = chart_table.build_table(self.A, self.manifest, fake_base)
        roll = table["rolls"]["R"]
        self.assertEqual(roll["film_base"]["rgb"], BASE)
        self.assertEqual(roll["film_base"]["frame"], "rolls/R/base.tif")
        self.assertEqual(sorted(roll["frames"]), ["2127", "2128"])
        patches = {p["id"]: p for p in roll["frames"]["2127"]["charts"][0]["patches"]}
        self.assertEqual(len(patches), 24)
        white = patches["A4"]  # the synthetic white sits at 2000 of 65535, grain aside
        t = 2000 / 65535
        self.assertAlmostEqual(white["transmission"][1], t, delta=t * 0.02)
        self.assertAlmostEqual(white["density"][1], -np.log10(t / BASE[1]), delta=0.01)
        self.assertEqual((white["role"], white["neutral_step"]), ("neutral", 0))
        self.assertEqual(white["reference_lab"], [95.19, -1.03, 2.93])
        self.assertEqual(patches["F1"]["role"], "saturated")
        self.assertLess(max(white["spread"]), 0.1)

    def test_neutrals_rise_with_exposure_and_a_fall_is_reported(self):
        table = chart_table.build_table(self.A, self.manifest, fake_base)
        self.assertEqual(chart_table.neutral_falls(table), [])
        frames = table["rolls"]["R"]["frames"]
        frames["2127"]["shot"], frames["2128"]["shot"] = (frames["2128"]["shot"],
                                                          frames["2127"]["shot"])
        falls = chart_table.neutral_falls(table)
        self.assertTrue(falls)
        self.assertIn("bracket b ccpp2-classic card 'ccpp2' A4", falls[0])

    def test_only_a_bracket_s_distinct_exposures_are_compared(self):
        table = chart_table.build_table(self.A, self.manifest, fake_base)
        frames = table["rolls"]["R"]["frames"]
        frames["2128"]["shot"] = {"exposure_ev": -1, "bracket": "b"}  # a repeat
        self.assertEqual(chart_table.neutral_falls(table), [])
        frames["2127"]["shot"], frames["2128"]["shot"] = {"exposure_ev": 0}, {"exposure_ev": -1}
        self.assertEqual(chart_table.neutral_falls(table), [])  # no bracket: no series

    def test_an_unexposed_frame_without_a_checksum_says_so(self):
        del self.manifest["rolls"]["R"]["frames"][0]["sha256"]
        with self.assertRaisesRegex(ValueError, "base.tif has no sha256"):
            chart_table.build_table(self.A, self.manifest, fake_base)

    def test_a_scan_changed_since_the_manifest_is_refused(self):
        self.manifest["rolls"]["R"]["frames"][1]["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "no longer matches its manifest sha256"):
            chart_table.build_table(self.A, self.manifest, fake_base)

    def test_a_check_under_an_older_layout_or_rules_is_refused(self):
        chk = self.manifest["rolls"]["R"]["frames"][1]["charts"][0]["check"]
        for key, value, why in (("target_version", 0, "layout version 0"),
                                ("rules", 0, "rules 0"), ("pass", False, "did not pass")):
            old = chk[key]
            chk[key] = value
            with self.assertRaisesRegex(ValueError, why):
                chart_table.build_table(self.A, self.manifest, fake_base)
            chk[key] = old

    def test_a_roll_with_charts_needs_one_unexposed_frame(self):
        self.manifest["rolls"]["R"]["frames"][0]["role"] = "leader"
        with self.assertRaisesRegex(ValueError, "0 unexposed frames"):
            chart_table.build_table(self.A, self.manifest, fake_base)

    def test_the_command_writes_a_byte_identical_table_beside_the_manifest(self):
        with open(os.path.join(self.A, "manifest.json"), "w") as f:
            json.dump(self.manifest, f)
        args = argparse.Namespace(asset_root=self.A, nc=None, out=None, dry_run=False)
        texts = []
        for _ in range(2):
            with mock.patch.object(chart_table, "measure_film_base", lambda nc, p: fake_base(p)), \
                    mock.patch("nctool.manifest.find_nc", return_value="hanten"), \
                    mock.patch.dict(os.environ, {}, clear=False) as env, \
                    contextlib.redirect_stdout(io.StringIO()), \
                    contextlib.redirect_stderr(io.StringIO()):
                env.pop("NC", None)  # a developer's $NC would be resolved first
                self.assertEqual(chart_table.cmd_table(args), 0)
            with open(os.path.join(self.A, chart_table.TABLE_NAME)) as f:
                texts.append(f.read())
        self.assertEqual(texts[0], texts[1])
        self.assertEqual(json.loads(texts[0])["table_version"], chart_table.TABLE_VERSION)


ASSETS = os.environ.get("NC_ASSET_ROOT", os.path.join(os.path.dirname(__file__),
                                                      "../../../../nc-assets"))
HANTEN = os.path.join(os.path.dirname(__file__), "../../../target/debug/hanten")


# Opt-in, like the Rust `#[ignore]` asset probes: it reads every labelled scan off the
# asset drive and runs `hanten measure-base`, so the ordinary gate stays fast and does
# not depend on what the archive holds today.
@unittest.skipUnless(os.environ.get("NCTOOL_ARCHIVE_TESTS") and np is not None
                     and os.path.exists(os.path.join(ASSETS, "manifest.json"))
                     and os.path.exists(HANTEN),
                     "opt-in: NCTOOL_ARCHIVE_TESTS=1, with ../nc-assets and target/debug/hanten")
class TestOnTheArchive(unittest.TestCase):
    def test_every_neutral_rises_with_exposure_across_each_bracket(self):
        from nctool import manifest
        data, err = manifest.load_manifest(os.path.join(ASSETS, "manifest.json"))
        self.assertIsNone(err)
        table = chart_table.build_table(
            os.path.abspath(ASSETS), data,
            lambda p: chart_table.measure_film_base(os.path.abspath(HANTEN), p))
        self.assertTrue(table["rolls"], "no labelled charts on the archive")
        self.assertEqual(chart_table.neutral_falls(table), [])


if __name__ == "__main__":
    unittest.main()
