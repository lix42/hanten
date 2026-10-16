"""Tests for nctool.cast: the chart-cast grid, patch sampling and summaries."""
from __future__ import annotations

import contextlib
import io
import json
import os
import stat
import tempfile
import types
import unittest
from pathlib import Path

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


def entry(card: str = "c1") -> dict:
    """A chart entry at `CORNERS` whose check vouches for it today."""
    t = charts.load_target(CLASSIC)
    return {"target": CLASSIC, "card": card, "corners": CORNERS,
            "check": {"pass": True, "rules": charts.CHECK_VERSION,
                      "target_version": t.get("version")}}


def p3_to_xyz_d50():
    from nctool import metrics
    m = metrics._mat_mul(metrics.bradford_adaptation("d65", "d50"),
                         metrics.rgb_to_xyz_d65(metrics.SPACES["display-p3"]))
    return np.asarray(m, dtype=np.float64)


class TestGrid(unittest.TestCase):
    def test_the_default_grid(self):
        configs = cast.expand_grid(cast.DEFAULT_GRID)
        # direct + 3 placements x 2 desaturations x (colour correction off + 2 forms x 4 lines)
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

    def test_graft_takes_the_chart_frames_whole_entries_and_leaves_the_input_alone(self):
        chart_entry = {"tones": "chart", "white_stops": 1.2, "exposure": 0.3}
        mine = {"params": {"roll": {"white_balance": [1, 1, 1.25],
                                    "frames": {"1.tif": {"tones": "pic"}}}}}
        both = {"params": {"roll": {"white_balance": [0.97, 1, 1.21],
                                    "frames": {"1.tif": {"tones": "other"},
                                               "9.tif": chart_entry}}}}
        out = cast.graft_frames(mine, both, ["9.tif"])
        self.assertEqual(out["params"]["roll"]["frames"],
                         {"1.tif": {"tones": "pic"}, "9.tif": chart_entry})
        self.assertEqual(out["params"]["roll"]["white_balance"], [1, 1, 1.25])
        self.assertNotIn("9.tif", mine["params"]["roll"]["frames"])
        with self.assertRaisesRegex(ValueError, "no entry for 8.tif"):
            cast.graft_frames(mine, both, ["8.tif"])

    def test_graft_refuses_a_recipe_without_a_roll_section(self):
        with self.assertRaisesRegex(ValueError, "has no params.roll.frames"):
            cast.graft_frames({"params": {"roll": {}}}, {"params": {}}, ["9.tif"])

    def test_balance_divergence_is_the_largest_channel_difference(self):
        mine = {"params": {"roll": {"white_balance": [1.0, 1.0, 1.25]}}}
        both = {"params": {"roll": {"white_balance": [0.98, 1.0, 1.20]}}}
        _, _, diff = cast.balance_divergence(mine, both)
        self.assertAlmostEqual(diff, 0.04)

    def test_duplicate_chart_stems_are_refused(self):
        with self.assertRaisesRegex(ValueError, "share a file stem \\(1\\)"):
            cast.check_stems([{"file": "a/1.tif"}, {"file": "b/1.tiff"}, {"file": "a/2.tif"}])
        cast.check_stems([{"file": "a/1.tif"}, {"file": "a/2.tif"}])

    def test_a_render_without_a_film_base_entry_is_a_value_error(self):
        with tempfile.TemporaryDirectory() as work:
            recipe = os.path.join(work, "r.json")
            with open(recipe, "w") as f:
                json.dump({"params": {"calibration": {"film_base": {"region": [0, 0, 1, 1]}}}}, f)
            with self.assertRaisesRegex(ValueError, "film_base.explicit"):
                cast.render_commands("hanten", "/A", {"chart": []}, work, {}, {"gain": recipe})

    def test_a_build_before_destinations_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            for version, ok in ((7, False), (17, True)):
                fake = os.path.join(d, f"hanten{version}")
                with open(fake, "w") as f:
                    f.write(f"#!/bin/sh\necho 'hanten 0.1.0'\necho 'pipeline_version: {version} (x)'\n")
                os.chmod(fake, os.stat(fake).st_mode | stat.S_IEXEC)
                if ok:
                    self.assertIn("pipeline_version: 17", cast.check_destination_build(fake))
                else:
                    with self.assertRaisesRegex(ValueError, "predates destinations"):
                        cast.check_destination_build(fake)

    def test_a_second_render_into_a_used_work_folder_replaces_the_first(self):
        # A fake measure-roll that, like hanten's, refuses an existing --out without --force.
        with tempfile.TemporaryDirectory() as d:
            fake = os.path.join(d, "hanten")
            with open(fake, "w") as f:
                f.write("#!/bin/sh\nout=''; force=''\nwhile [ $# -gt 0 ]; do\n"
                        "  case $1 in --out) out=$2; shift;; --force) force=1;; esac; shift\n"
                        "done\n[ -e \"$out\" ] && [ -z \"$force\" ] && exit 2\n"
                        "echo '{\"params\": {\"roll\": {\"white_balance\": [1, 1, 1], "
                        "\"frames\": {\"9.tif\": {}}}}}' > \"$out\"\n")
            os.chmod(fake, os.stat(fake).st_mode | stat.S_IEXEC)
            work = os.path.join(d, "work")
            frames = {"real": [{"file": "1.tif"}], "chart": [{"file": "9.tif"}]}
            os.makedirs(os.path.join(work, "renders", "old"))
            cast.write_record(work, {"roll": "R", "complete": True})
            for _ in range(2):
                cast.check_work(work)
                with contextlib.redirect_stdout(io.StringIO()):
                    recipes = cast.prepare_recipes(fake, d, frames, work, ["gain"])
                cast.reset_work(work, "R")
            self.assertTrue(os.path.exists(recipes["gain"]))
            self.assertFalse(os.path.exists(os.path.join(work, "renders")))
            with open(os.path.join(work, cast.CONFIGS_NAME)) as f:
                self.assertEqual(json.load(f), {"roll": "R", "complete": False})

    def test_renders_without_a_record_are_not_cleared(self):
        with tempfile.TemporaryDirectory() as work:
            os.makedirs(os.path.join(work, "renders", "mine"))
            with self.assertRaisesRegex(ValueError, "pick an empty folder"):
                cast.reset_work(work, "R")
            self.assertTrue(os.path.isdir(os.path.join(work, "renders", "mine")))

    def test_a_failing_roll_measurement_leaves_the_earlier_run_alone(self):
        with tempfile.TemporaryDirectory() as d:
            fake = os.path.join(d, "hanten")
            with open(fake, "w") as f:
                f.write("#!/bin/sh\nif [ \"$1\" = --version ]; then\n  echo 'hanten 0.1.0'\n"
                        "  echo 'pipeline_version: 17 (x)'\n  exit 0\nfi\n"
                        "echo 'measure-roll: boom' >&2\nexit 2\n")
            os.chmod(fake, os.stat(fake).st_mode | stat.S_IEXEC)
            root = os.path.join(d, "assets")
            os.makedirs(root)
            with open(os.path.join(root, "manifest.json"), "w") as f:
                json.dump({"rolls": {"R": {"frames": [{"file": "1.tif"},
                                                      {"file": "9.tif", "role": "chart"}]}}}, f)
            work = os.path.join(d, "work")
            os.makedirs(os.path.join(work, "renders", "old"))
            cast.write_record(work, {"roll": "R", "complete": True, "configurations": ["old"]})
            args = types.SimpleNamespace(asset_root=root, roll="R", nc=fake, grid=None,
                                         work=work, jobs=1)
            err = io.StringIO()
            with contextlib.redirect_stderr(err), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(cast.cmd_render(args), 2)
            self.assertIn("boom", err.getvalue())
            self.assertTrue(os.path.isdir(os.path.join(work, "renders", "old")))
            self.assertTrue(cast.read_configs(work, "R")["complete"])

    def test_warnings_keep_file_names_whole(self):
        lines = cast.warning_summary(["w: /a/2127.tif: lost 12 (0.5%)",
                                      "w: /a/2127.tif: lost 7 (0.1%)", "w: /a/2128.tif: lost 7"])
        self.assertEqual(lines, ["2x w: /a/2127.tif: lost # (#%)", "1x w: /a/2128.tif: lost 7"])

    def test_warnings_are_grouped_by_wording(self):
        self.assertEqual(cast.warning_summary(["w: a\nw: lost 12 (0.5%)", "", "w: a",
                                               "w: lost 7 (0.1%)"]),
                         ["2x w: a", "2x w: lost # (#%)"])

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
        self.entries = [entry()]

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

    def test_the_matched_frame_is_the_one_whose_d4_lands_nearest_the_reference(self):
        dark = self.rows(frame="dark")
        d4 = next(r["lab"][0] for r in dark if r["patch"] == "D4")
        ref_l = self.target["reference"]["values"]["D4"][0]
        # Shift a copy so its D4 lands exactly on the reference: it must be the match.
        on = [dict(r, frame="on", lab=[r["lab"][0] + ref_l - d4, *r["lab"][1:]]) for r in dark]
        m = cast.summarize(dark + on)["matched"]
        self.assertEqual((m["frame"], m["card"], m["patch"]), ("on", "c1", "D4"))
        self.assertAlmostEqual(m["l"], ref_l)

    def test_a_second_card_on_one_frame_is_matched_on_its_own(self):
        one = self.rows()
        d4 = next(r["lab"][0] for r in one if r["patch"] == "D4")
        ref_l = self.target["reference"]["values"]["D4"][0]
        other = [dict(r, card="c2", lab=[r["lab"][0] + ref_l - d4, *r["lab"][1:]]) for r in one]
        m = cast.summarize(one + other)["matched"]
        self.assertEqual((m["frame"], m["card"]), ("1", "c2"))

    def test_a_stale_chart_entry_is_refused(self):
        entry = dict(self.entries[0], check={"pass": False})
        with self.assertRaisesRegex(ValueError, "on card 'c1': its check did not pass; re-run"):
            cast.patch_rows(np.zeros((60, 80, 3)), p3_to_xyz_d50(), "1", [entry])

    def test_colour_accuracy_is_zero_at_the_reference(self):
        rows = [{"frame": "1", "role": "colour", "patch": "A", "lab": [50, 40, 20],
                 "ref": [50, 40, 20]},
                {"frame": "1", "role": "colour", "patch": "B", "lab": [50, 10, 10],
                 "ref": [50, 10, 10]}]
        acc = cast.colour_accuracy(rows)
        self.assertAlmostEqual(acc["hue_error"], 0.0)
        self.assertAlmostEqual(acc["chroma_ratio"], 1.0)
        self.assertAlmostEqual(acc["delta_e"], 0.0)


@unittest.skipIf(np is None, "numpy not installed")
class TestMeasureFiles(unittest.TestCase):
    """`measure` end to end: a manifest, `render`'s record and 16-bit TIFF renders."""

    SHAPE = (300, 400)

    def setUp(self):
        from nctool.test_reference import v2_profile
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name, "assets")
        self.work = Path(self.tmp.name, "work")
        self.profile = v2_profile("display-p3", gamma=1.0)
        self.target = charts.load_target(CLASSIC)
        rows, cols = self.SHAPE
        self.frames = [{"file": "rolls/R/1.tif", "role": "chart", "width": cols,
                        "height": rows, "charts": [entry()]}]
        self.write_manifest()
        self.write_configs({"roll": "R", "complete": True, "build": "hanten 0.1.0\ncommit: abc",
                            "configurations": ["c"], "density_scale": None})
        self.write_render(self.work / "renders" / "c" / "1.tif")

    def tearDown(self):
        self.tmp.cleanup()

    def write_manifest(self):
        self.root.mkdir(parents=True, exist_ok=True)
        (self.root / "manifest.json").write_text(json.dumps(
            {"rolls": {"R": {"frames": self.frames}}}))

    def write_configs(self, rec):
        self.work.mkdir(parents=True, exist_ok=True)
        (self.work / cast.CONFIGS_NAME).write_text(json.dumps(rec))

    def write_render(self, path: Path, tint=(1.0, 1.0, 1.0)):
        import tifffile
        greys = {pid: [0.03 + 0.12 * i * t for t in tint]   # under 1, so no clip
                 for i, pid in enumerate(reversed(self.target["neutrals"]))}
        img = painted(self.target, CORNERS, greys, shape=self.SHAPE)
        path.parent.mkdir(parents=True, exist_ok=True)
        tags = [(34675, 7, len(self.profile), self.profile, True)]
        with tifffile.TiffWriter(str(path)) as tif:
            tif.write(np.round(img * 65535).astype(np.uint16), photometric="rgb",
                      extratags=tags)

    def measure(self, extra=()):
        return cast.measure(str(self.root), "R", str(self.work), list(extra))

    def test_a_neutral_render_reads_neutral_through_its_profile(self):
        self.write_render(Path(self.tmp.name, "ext", "1.tif"), tint=(1.0, 1.0, 1.3))
        out = self.measure([f"EXT={Path(self.tmp.name, 'ext')}"])
        mine, ext = out["sources"]["hanten:c"], out["sources"]["EXT"]
        self.assertEqual(mine["space"], ["linear-display-p3"])
        self.assertLess(mine["summary"]["all"]["c"], 0.1)
        self.assertLess(ext["summary"]["all"]["b"], -3)
        self.assertEqual({r["card"] for r in mine["patches"]}, {"c1"})

    def test_an_unfinished_render_record_is_refused(self):
        self.write_configs({"roll": "R", "complete": False})
        with self.assertRaisesRegex(ValueError, "did not finish"):
            self.measure()

    def test_the_build_is_carried_into_the_cast_file_and_printed(self):
        out = self.measure()
        self.assertEqual(out["build"], "hanten 0.1.0\ncommit: abc")
        text = io.StringIO()
        cast.print_table(out, file=text)
        self.assertIn("build: hanten 0.1.0; commit: abc", text.getvalue())

    def table(self, sha):
        e = entry()
        patches = [{"role": "neutral", "density": [g, g, g]} for g in (0.3, 0.5, 0.7)]
        (self.root / "charts.json").write_text(json.dumps({"rolls": {"R": {"frames": {"1": {
            "file": "rolls/R/1.tif", "sha256": sha,
            "charts": [{"target": e["target"], "card": e["card"], "check": e["check"],
                        "patches": patches}]}}}}}))
        self.frames[0]["sha256"] = "aa"
        self.write_manifest()
        self.write_configs({"roll": "R", "complete": True, "configurations": ["c"],
                            "density_scale": [1, 1, 1]})

    def test_a_current_chart_table_is_read(self):
        self.table("aa")
        self.assertEqual(self.measure()["density"]["offset"]["n"], 3)

    def test_the_tables_chart_order_does_not_matter(self):
        self.table("aa")
        second = dict(entry(), card="c2")
        self.frames[0]["charts"] = [second, entry()]   # the manifest in reverse order
        self.write_manifest()
        path = self.root / "charts.json"
        doc = json.loads(path.read_text())
        charts_ = doc["rolls"]["R"]["frames"]["1"]["charts"]
        charts_.append(dict(charts_[0], card="c2"))     # the table sorted by (target, card)
        path.write_text(json.dumps(doc))
        self.assertEqual(self.measure()["density"]["offset"]["n"], 6)

    def test_a_stale_chart_table_is_refused(self):
        self.table("bb")
        with self.assertRaisesRegex(ValueError, "charts.json is stale for roll R: .*other "
                                                "bytes.*nctool manifest charts table"):
            self.measure()

    def test_an_export_not_at_the_scans_size_is_refused(self):
        self.frames[0]["width"] = 401
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, "is 400x300, the scan 401x300"):
            self.measure()

    def test_a_missing_render_record_is_refused(self):
        (self.work / cast.CONFIGS_NAME).unlink()
        with self.assertRaisesRegex(ValueError, "run `nctool manifest charts cast render`"):
            self.measure()

    def test_only_the_recorded_configurations_are_read(self):
        self.write_render(self.work / "renders" / "stale" / "1.tif")
        self.assertEqual(list(self.measure()["sources"]), ["hanten:c"])

    def test_a_record_of_another_roll_is_refused(self):
        self.write_configs({"roll": "S", "complete": True, "configurations": ["c"]})
        with self.assertRaisesRegex(ValueError, "records roll 'S'"):
            self.measure()

    def test_a_repeated_or_colliding_source_name_is_refused(self):
        ext = str(Path(self.tmp.name, "ext"))
        for extra in ([f"A={ext}", f"A={ext}"], [f"hanten:c={ext}"]):
            with self.assertRaisesRegex(ValueError, "repeats a source name"):
                self.measure(extra)

    def test_duplicate_chart_stems_are_refused(self):
        self.frames.append(dict(self.frames[0], file="rolls/R/b/1.tiff"))
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, "share a file stem"):
            self.measure()

    def test_an_unreadable_export_is_an_error_at_the_cli(self):
        args = types.SimpleNamespace(asset_root=str(self.root), roll="R", work=str(self.work),
                                     source=[f"X={self.tmp.name}/nowhere"], out=None)
        err = io.StringIO()
        with contextlib.redirect_stderr(err), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(cast.cmd_measure(args), 2)
        self.assertIn("error: X:", err.getvalue())


class TestDensity(unittest.TestCase):
    def test_a_constant_blue_offset_is_recovered_with_unit_density_scales(self):
        scale = [1.0, 0.8, 0.5]
        patches = [{"role": "neutral", "density": [g, g / scale[1], (g - 0.04) / scale[2]]}
                   for g in (0.25, 0.45, 0.65, 0.85, 1.2)]
        frames = {"1": {"charts": [{"patches": patches + [{"role": "colour",
                                                           "density": [9, 9, 9]}]}]}}
        d = cast.density_residuals(frames, scale)
        self.assertAlmostEqual(d["offset"]["blue"], 0.04)
        self.assertAlmostEqual(d["offset"]["red"], 0.0)
        self.assertEqual(d["offset"]["n"], 4)   # 1.2 lies past DENSITY_RANGE
        self.assertAlmostEqual(d["scale_fit"]["red"], 1.0)
        self.assertAlmostEqual(d["scale_fit"]["blue"], 1.0)

    def test_no_greys_gives_nothing(self):
        self.assertEqual(cast.density_residuals({}, [1, 1, 1]), {})

    def frames(self, greens, red=lambda g: g):
        return {"1": {"charts": [{"patches": [{"role": "neutral", "density": [red(g), g, g]}
                                              for g in greens]}]}}

    def test_the_density_scale_is_fitted_over_the_range_and_the_shoulder_apart(self):
        # Red rises 1:1 up to the shoulder, then 1.2:1.
        top = cast.SHOULDER_FROM
        red = lambda g: g if g < top else top + 1.2 * (g - top)
        greens = [0.1, 0.3, 0.5, 0.7, top, top + 0.2, top + 0.4]
        d = cast.density_residuals(self.frames(greens, red), [1, 1, 1])
        straight = [g for g in greens if cast.DENSITY_RANGE[0] <= g < cast.DENSITY_RANGE[1]]
        self.assertEqual(d["scale_fit"]["n"], len(straight))
        self.assertEqual(d["shoulder"]["n"], 3)
        self.assertAlmostEqual(d["shoulder"]["red"], 1.2)
        self.assertEqual([round(g, 6) for g in d["shoulder"]["green"]], [top, top + 0.4])
        # The toe (0.1) is outside the fit: with the shoulder's points removed it is 1:1.
        flat = cast.density_residuals(self.frames([0.1, 0.3, 0.5, 0.7], lambda g: g if g > 0.2
                                                  else 0.5), [1, 1, 1])
        self.assertAlmostEqual(flat["scale_fit"]["red"], 1.0)

    def test_a_value_on_a_bin_edge_lands_in_its_own_bin(self):
        d = cast.density_residuals(self.frames([0.6, 0.2 * 3]), [1, 1, 1])
        self.assertEqual(list(d["bins"]), ["0.6"])

    def test_one_grey_or_flat_green_gives_no_density_scale(self):
        d = cast.density_residuals(self.frames([0.5]), [1, 1, 1])
        self.assertIsNone(d["scale_fit"]["red"])
        self.assertIsNone(d["shoulder"]["red"])
        d = cast.density_residuals(self.frames([0.5, 0.5]), [1, 1, 1])
        self.assertIsNone(d["scale_fit"]["blue"])

    def test_the_table_prints_a_marker_where_there_is_no_number(self):
        d = cast.density_residuals(self.frames([1.5]), [1, 1, 1])   # no grey in the range
        self.assertIsNone(d["offset"]["red"])
        out = io.StringIO()
        cast.print_table({"sources": {"x": {"summary": {}}}, "density": d}, file=out)
        self.assertIn("offset to cancel red       -", out.getvalue())


if __name__ == "__main__":
    unittest.main()
