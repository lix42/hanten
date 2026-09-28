"""Hermetic tests for manifest-driven roll conversion and analysis."""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from nctool import roll  # noqa: E402


def _frame(path: str, role: str, width=100, height=80) -> dict:
    return {"file": path, "role": role, "width": width, "height": height,
            "sha256": hashlib.sha256(b"x").hexdigest(), "bytes": 1}


class TestRecipe(unittest.TestCase):
    def test_partial_recipe_recursively_overlays_defaults(self):
        merged = roll._deep_merge(
            {"print": {"print_exposure": 0, "black_point": 0}, "output": {"preset": "x"}},
            {"print": {"print_exposure": 1}})
        self.assertEqual(merged["print"], {"print_exposure": 1, "black_point": 0})
        self.assertEqual(merged["output"], {"preset": "x"})

    def test_switching_tagged_curve_replaces_incompatible_keys(self):
        merged = roll._deep_merge(
            {"curve": {"type": "exponential", "gamma": 2, "anchor": {"mid-at-base-offset": .62}}},
            {"curve": {"type": "characteristic", "stock": "ektar-100"}})
        self.assertEqual(merged["curve"], {"type": "characteristic", "stock": "ektar-100"})

    def test_a_retired_curve_tag_is_dropped_only_where_the_build_writes_none(self):
        # This build's defaults carry no curve tag, so an old recipe's
        # `"type": "exponential"` must not read as a variant switch that drops the
        # default curve's keys.
        partial = {"reconstruction": {"curve": {"type": "exponential", "gamma": 1.5}}}
        defaults = {"reconstruction": {"curve": {"gamma": 2.0,
                                                 "anchor": {"mid-at-base-offset": .62}}}}
        roll._drop_retired_curve_tag(defaults, partial)
        merged = roll._deep_merge(defaults, partial)
        self.assertEqual(merged["reconstruction"]["curve"],
                         {"gamma": 1.5, "anchor": {"mid-at-base-offset": .62}})
        # The reference build writes the tag, and there it is a selector: kept.
        partial = {"reconstruction": {"curve": {"type": "exponential", "gamma": 1.5}}}
        roll._drop_retired_curve_tag(
            {"reconstruction": {"curve": {"type": "sigmoid"}}}, partial)
        self.assertEqual(partial["reconstruction"]["curve"]["type"], "exponential")
        # A malformed section is left untouched for `_freeze_recipe` to refuse.
        for partial in ({"reconstruction": "invalid"}, {"reconstruction": {"curve": 1}}):
            roll._drop_retired_curve_tag(defaults, partial)
        _, error = roll._freeze_recipe({"reconstruction": "invalid"}, [.1, .2, .3],
                                       None, None, None)
        self.assertIn("must be an object", error)

    def test_measured_values_override_recipe_calibration(self):
        base = {
            "recipe_version": 2,
            "calibration": {"film_base": {"explicit": [9, 9, 9]}},
            "output": {"display": {"gamut": "display-p3"}},
        }
        recipe, error = roll._freeze_recipe(base, [.1, .2, .3], "chromogenic",
                                             {"range": "hdr", "container": "jpeg"}, .5)
        self.assertIsNone(error)
        self.assertEqual(recipe["calibration"], {"film_base": {"explicit": [.1, .2, .3]}})
        self.assertEqual(recipe["input"]["film_type"], "chromogenic")
        # Stated axes merge over the recipe's; unstated ones stay nc's to derive.
        self.assertEqual(recipe["output"], {"display": {
            "gamut": "display-p3", "range": "hdr", "container": "jpeg"}})
        self.assertEqual(recipe["scene_correction"]["exposure"], .5)

    def test_the_film_master_replaces_a_display_destination_and_back(self):
        base = {"recipe_version": 2, "output": {"display": {"range": "hdr"}}}
        recipe, error = roll._freeze_recipe(base, [.1, .2, .3], None, "film-master")
        self.assertIsNone(error)
        self.assertEqual(recipe["output"], "film-master")
        recipe, error = roll._freeze_recipe(recipe, [.1, .2, .3], None, {"range": "sdr"})
        self.assertIsNone(error)
        self.assertEqual(recipe["output"], {"display": {"range": "sdr"}})

    def test_the_destination_flags_contradicting_each_other_are_refused(self):
        args = argparse.Namespace(film_master=True, range="hdr", transfer=None,
                                  gamut=None, container=None)
        output, error = roll._output_override(args)
        self.assertIsNone(output)
        self.assertIn("--range", error)
        args.range = None
        self.assertEqual(roll._output_override(args), ("film-master", None))
        args.film_master = False
        self.assertEqual(roll._output_override(args), (None, None))

    # A preset build's recipe (the reference build) has no destination or scene
    # correction to set; its own keys come through the partial --recipe.
    def test_the_destination_flags_are_refused_on_a_preset_builds_recipe(self):
        for output, exposure in (("film-master", None), ({"range": "hdr"}, None),
                                 (None, .5)):
            recipe, error = roll._freeze_recipe({"output": {"preset": "display-p3"}},
                                                [.1, .2, .3], None, output, exposure)
            self.assertIsNone(recipe)
            self.assertIn("recipe_version 2", error)
        recipe, error = roll._freeze_recipe({"output": {"preset": "display-p3"}},
                                            [.1, .2, .3], None)
        self.assertIsNone(error)
        self.assertEqual(recipe["output"], {"preset": "display-p3"})

    def test_the_reference_builds_retired_dmax_default_is_dropped(self):
        # The reference build's `hanten params` still writes `"dmax": "fixed"`; a
        # stated reference is kept for `hanten` to refuse by name.
        recipe, error = roll._freeze_recipe(
            {"calibration": {"film_base": None, "dmax": "fixed"}}, [.1, .2, .3],
            None, None, None)
        self.assertIsNone(error)
        self.assertNotIn("dmax", recipe["calibration"])
        recipe, error = roll._freeze_recipe(
            {"calibration": {"dmax": {"explicit": 1.4}}}, [.1, .2, .3], None, None, None)
        self.assertIsNone(error)
        self.assertEqual(recipe["calibration"]["dmax"], {"explicit": 1.4})

    def test_an_unstated_curve_is_left_to_the_build(self):
        # `hanten params` writes the build's own curve into the merged base; freezing
        # adds no tag this build no longer writes.
        recipe, error = roll._freeze_recipe({}, [.1, .2, .3], None, None, None)
        self.assertIsNone(error)
        self.assertNotIn("curve", recipe["reconstruction"])
        _, error = roll._freeze_recipe({"reconstruction": {"curve": 1}}, [.1, .2, .3],
                                       None, None, None)
        self.assertIn("must be an object", error)

    def test_default_region_is_center_eighty_percent(self):
        value, error = roll._region(None, {"width": 100, "height": 80}, "Dmin")
        self.assertIsNone(error)
        self.assertEqual(value, "10,8,80,64")

class TestConvert(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        frames = [
            _frame("rolls/R/u.tif", "unexposed"),
            _frame("rolls/R/l.tif", "leader"),
            _frame("rolls/R/a.tif", "real"),
            _frame("rolls/R/b.tif", "real"),
        ]
        for frame in frames:
            path = self.root / frame["file"]
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"x")
        (self.root / "manifest.json").write_text(json.dumps({
            "schema_version": 1, "generated": "today",
            "rolls": {"R": {"frames": frames}}, "converted": {}, "samples": [],
        }))

    def args(self, **updates):
        values = dict(asset_root=str(self.root), roll="R", nc="fake-nc", config="test",
                      out_dir=None, recipe=None, dmin_region=None, dmin_mode="grid",
                      film_type=None, film_master=False, range=None, transfer=None,
                      gamut=None, container=None, exposure=None,
                      max_memory="1GiB", strict_estimate=False, strict_roll=False)
        values.update(updates)
        return argparse.Namespace(**values)

    #: `hanten params` from a build that takes destinations (`recipe_version` 2).
    DEFAULTS = {
        "recipe_version": 2,
        "calibration": {"film_base": None},
        "reconstruction": {"scale": [1.0, 0.84, 0.73], "linearization": 1.8,
                           "anchor": {"mid-at-base-offset": .62}},
        "scene_correction": {"exposure": 0.0},
        "output": {"display": {}},
    }

    #: `hanten params` from a build that takes presets (the reference build).
    PRESET_DEFAULTS = {
        "reconstruction": {"schema_version": 1,
                           "curve": {"type": "exponential", "gamma": 2.0,
                                     "anchor": {"mid-at-base-offset": .62}}},
        "calibration": {"film_base": None},
        "print": {"print_exposure": 0},
        "output": {"preset": "gain-map-hdr", "depth": "u16"},
    }

    defaults = DEFAULTS

    def fake_run(self, argv, **_kwargs):
        if argv[1] == "params":
            return mock.Mock(returncode=0, stdout=json.dumps(self.defaults), stderr="")
        if argv[1] == "estimate":
            return mock.Mock(returncode=0, stdout=json.dumps({
                "film_base": {"r": .1, "g": .2, "b": .3}}), stderr="")
        self.assertEqual(argv[1], "roll")
        report_path = Path(argv[argv.index("--report-file") + 1])
        inputs = argv[2:argv.index("--out-dir")]
        report = {
            "identity": {"nc_version": "x", "pipeline_version": 8, "target": "test"},
            "frames": [
                {"input": path, "status": "ok", "output_stats": {"mean": [.1, .2, .3]},
                 "loss": {"total_samples": 3, "clipped_low": 0, "clipped_high": 0},
                 "identity": {"nc_version": "x", "pipeline_version": 8},
                 "chain": {"destination": "film-master"}}
                for path in inputs
            ],
            "summary": {"total": 2, "succeeded": 2, "failed": 0},
        }
        report_path.write_text(json.dumps(report))
        return mock.Mock(returncode=0, stdout="", stderr="")

    def test_converts_manifest_real_frames_and_writes_provenance(self):
        seen = []

        def capture(argv, **kwargs):
            seen.append(argv)
            return self.fake_run(argv, **kwargs)

        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(roll.subprocess, "run", side_effect=capture), \
             contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = roll.cmd_convert(self.args())
        self.assertEqual(code, 0, err.getvalue())
        run = self.root / "converted/nc/test/R"
        tags = json.loads((run / "tags.json").read_text())
        recipe = json.loads((run / "recipe.json").read_text())
        calibration = json.loads((run / "calibration.json").read_text())
        self.assertEqual(tags["roll"], "R")
        self.assertEqual(tags["summary"]["succeeded"], 2)
        self.assertEqual(recipe["calibration"], {"film_base": {"explicit": [.1, .2, .3]}})
        # Only the unexposed frame is estimated; a manifest leader is not measured.
        self.assertEqual(len([a for a in seen if a[1] == "estimate"]), 1)
        self.assertEqual(calibration["dmin"]["region"], "10,8,80,64")
        self.assertEqual(calibration["dmin"]["mode"], "grid")
        self.assertNotIn("dmax", calibration)
        self.assertEqual(tags["calibration"], {"dmin": {
            "frame": "rolls/R/u.tif", "region": "10,8,80,64", "mode": "grid",
            "value": [.1, .2, .3]}})
        self.assertEqual(json.loads(out.getvalue())["config"], "test")

    def test_the_destination_flags_reach_the_frozen_recipe(self):
        with mock.patch.object(roll.subprocess, "run", side_effect=self.fake_run), \
             contextlib.redirect_stdout(io.StringIO()), \
             contextlib.redirect_stderr(io.StringIO()) as err:
            code = roll.cmd_convert(self.args(config="hdr", range="hdr",
                                              container="jpeg", exposure=.5))
        self.assertEqual(code, 0, err.getvalue())
        recipe = json.loads((self.root / "converted/nc/hdr/R/recipe.json").read_text())
        self.assertEqual(recipe["recipe_version"], 2)
        self.assertEqual(recipe["output"], {"display": {"range": "hdr",
                                                        "container": "jpeg"}})
        self.assertEqual(recipe["scene_correction"]["exposure"], .5)

    # Refused before the Dmin estimate: everything it depends on is known by then.
    def test_the_destination_flags_on_a_preset_build_are_refused_before_measuring(self):
        self.defaults = self.PRESET_DEFAULTS
        with mock.patch.object(roll.subprocess, "run", side_effect=self.fake_run) as run, \
             contextlib.redirect_stdout(io.StringIO()), \
             contextlib.redirect_stderr(io.StringIO()) as err:
            code = roll.cmd_convert(self.args(film_master=True))
        self.assertEqual(code, 2)
        self.assertIn("recipe_version 2", err.getvalue())
        self.assertEqual(run.call_count, 1)  # `params` only

    def test_the_retired_density_tag_does_not_replace_the_default_reconstruction(self):
        self.defaults = self.PRESET_DEFAULTS
        recipe_path = self.root / "partial.json"
        recipe_path.write_text(json.dumps({"reconstruction": {
            "type": "density", "density": {"offset": [.1, 0, 0]}}}))
        with mock.patch.object(roll.subprocess, "run", side_effect=self.fake_run), \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            code = roll.cmd_convert(self.args(config="tagged", recipe=str(recipe_path)))
        self.assertEqual(code, 0)
        recipe = json.loads((self.root / "converted/nc/tagged/R/recipe.json").read_text())
        reconstruction = recipe["reconstruction"]
        self.assertNotIn("type", reconstruction)
        self.assertEqual(reconstruction["density"], {"offset": [.1, 0, 0]})
        self.assertEqual(reconstruction["curve"]["gamma"], 2.0)
        self.assertEqual(reconstruction["curve"]["anchor"], {"mid-at-base-offset": .62})

    def test_a_missing_leader_does_not_fail_the_run(self):
        (self.root / "rolls/R/l.tif").unlink()
        with mock.patch.object(roll.subprocess, "run", side_effect=self.fake_run), \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as err:
            code = roll.cmd_convert(self.args(config="noleader"))
        self.assertEqual(code, 0, err.getvalue())
        tags = json.loads((self.root / "converted/nc/noleader/R/tags.json").read_text())
        self.assertNotIn("rolls/R/l.tif", [f["file"] for f in tags["source_frames"]])

    def test_region_mode_omits_grid_flag(self):
        seen = []

        def capture(argv, **kwargs):
            seen.append(argv)
            return self.fake_run(argv, **kwargs)

        with mock.patch.object(roll.subprocess, "run", side_effect=capture), \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            code = roll.cmd_convert(self.args(config="region", dmin_mode="region"))
        self.assertEqual(code, 0)
        dmin_argv = next(argv for argv in seen if argv[1] == "estimate")
        self.assertNotIn("--grid", dmin_argv)

    def test_refuses_nonempty_output_before_roll(self):
        run = self.root / "converted/nc/test/R"
        run.mkdir(parents=True)
        (run / "old.jpg").write_bytes(b"old")
        with mock.patch.object(roll.subprocess, "run", side_effect=self.fake_run) as run_mock:
            code = roll.cmd_convert(self.args())
        # Calibration precedes config hashing/output resolution, but the existing
        # directory is still refused before hanten roll can overwrite an artifact.
        self.assertEqual(code, 2)
        # `params` and the Dmin estimate.
        self.assertEqual(run_mock.call_count, 2)


class TestAnalyze(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def write_run(self, config: str, mean: list[float], clipped: int = 0,
                  destination=None):
        """A converted roll: a reference-build (preset) run, or — given the
        `destination` its frames resolved — a destination build's."""
        base = self.root / "converted/nc" / config / "R"
        base.mkdir(parents=True)
        frame = {"input": "/assets/rolls/R/a.tif",
                 "output": "/outputs/a_positive.tiff", "status": "ok",
                 "film_base": {"r": .1, "g": .2, "b": .3},
                 "output_stats": {"mean": mean},
                 "loss": {"total_samples": 100, "clipped_low": 0,
                          "clipped_high": clipped},
                 "memory": {"detected_total_ram_bytes": 123},
                 "identity": {"pipeline_version": 8}}
        if destination is None:
            frame.update(dmax=1.3, identity={"params_hash": config})
            recipe = {"output": {"preset": "legacy"}}
        else:
            frame["chain"] = {"destination": destination}
            recipe = {"recipe_version": 2, "output": {"display": {}}}
        report = {"identity": {"pipeline_version": 7 if destination is None else 8},
                  "summary": {"total": 1, "succeeded": 1, "failed": 0},
                  "frames": [frame]}
        (base / "roll-report.json").write_text(json.dumps(report))
        (base / "tags.json").write_text(json.dumps({
            "schema_version": 1, "kind": "nctool-roll-conversion", "roll": "R",
            "config": config, "report_file": f"converted/nc/{config}/R/roll-report.json",
            "source_frames": [{"file": "rolls/R/a.tif", "role": "real", "sha256": "x"}],
            "recipe": recipe,
            "calibration": {"dmin": {"value": [.1, .2, .3]}},
            "identity": frame["identity"],
        }))

    def test_writes_stable_analysis_beside_tags(self):
        self.write_run("a", [.1, .2, .3])
        args = argparse.Namespace(asset_root=str(self.root), roll="R", run="a", out=None)
        with contextlib.redirect_stderr(io.StringIO()):
            code = roll.cmd_analyze(args)
        self.assertEqual(code, 0)
        path = self.root / "converted/nc/a/R/analysis.json"
        first = path.read_bytes()
        result = json.loads(first)
        self.assertEqual(result["kind"], "nctool-roll-analysis")
        self.assertEqual(result["frames"][0]["source"], "rolls/R/a.tif")
        self.assertEqual(result["frames"][0]["output_stats"]["mean"], [.1, .2, .3])
        self.assertEqual(result["frames"][0]["clip_fraction"], 0.0)
        self.assertNotIn("input", result["frames"][0])
        self.assertNotIn("output", result["frames"][0])
        self.assertNotIn("memory", result["frames"][0])

        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(roll.cmd_analyze(args), 0)
        self.assertEqual(path.read_bytes(), first)

    # A destination build's recipe may leave the axes to nc, so the depth comes from
    # what each frame reports it resolved.
    def test_a_destination_runs_depth_follows_its_resolved_destination(self):
        for config, destination, depth in (
                ("m", "film-master", "f32"),
                ("j", {"display": {"range": "hdr", "transfer": "native",
                                   "gamut": "display-p3", "container": "jpeg"}}, "u8")):
            self.write_run(config, [.1, .2, .3], destination=destination)
            args = argparse.Namespace(asset_root=str(self.root), roll="R", run=config,
                                      out=None)
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(roll.cmd_analyze(args), 0)
            result = json.loads(
                (self.root / f"converted/nc/{config}/R/analysis.json").read_text())
            self.assertEqual(result["output_depth"], depth)
            self.assertEqual(result["frames"][0]["chain"], {"destination": destination})

    def edit_report(self, config: str, edit) -> None:
        path = self.root / f"converted/nc/{config}/R/roll-report.json"
        report = json.loads(path.read_text())
        edit(report)
        path.write_text(json.dumps(report))

    def analyze(self, config: str) -> tuple[int, str]:
        args = argparse.Namespace(asset_root=str(self.root), roll="R", run=config,
                                  out=None)
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = roll.cmd_analyze(args)
        return code, err.getvalue()

    def test_a_mixed_depth_roll_records_each_frames_depth(self):
        tiff = {"display": {"range": "sdr", "transfer": "native",
                            "gamut": "display-p3", "container": "tiff"}}
        self.write_run("mixed", [.1, .2, .3], destination="film-master")

        def add_frames(report):
            first = report["frames"][0]
            report["frames"] += [
                dict(first, input="/assets/rolls/R/b.tif", chain={"destination": tiff}),
                {"input": "/assets/rolls/R/c.tif", "status": "failed", "error": "x"}]
        self.edit_report("mixed", add_frames)
        self.assertEqual(self.analyze("mixed")[0], 0)
        result = json.loads(
            (self.root / "converted/nc/mixed/R/analysis.json").read_text())
        self.assertEqual(result["output_depth"], "mixed")
        self.assertEqual(
            {frame["source"]: frame.get("output_depth") for frame in result["frames"]},
            {"rolls/R/a.tif": "f32", "b.tif": "u16", "c.tif": None})

    def test_a_depth_that_cannot_be_stated_is_refused_not_left_empty(self):
        # Like `metrics.space_for_run`, analysis refuses rather than write no depth.
        # A destination build with no resolved destination must not fall back to the
        # reference build's preset default (`gain-map-hdr`, u8).
        def all_failed(report):
            report["frames"] = [{"input": "/assets/rolls/R/a.tif", "status": "failed",
                                 "error": "x"}]

        def unknown(report):
            report["frames"][0]["chain"] = {
                "destination": {"display": {"container": "webp"}}}

        def no_build(report):
            del report["identity"]

        for config, edit, wording in (
                ("failed", all_failed, "no frame resolved a destination"),
                ("unknown", unknown, "unknown depth"),
                ("nobuild", no_build, "identity.pipeline_version")):
            self.write_run(config, [.1, .2, .3], destination="film-master")
            self.edit_report(config, edit)
            code, err = self.analyze(config)
            self.assertEqual(code, 2)
            self.assertIn(wording, err)
            self.assertFalse(
                (self.root / f"converted/nc/{config}/R/analysis.json").exists())

    def test_a_preset_build_with_every_frame_failed_takes_its_presets_depth(self):
        self.write_run("p", [.1, .2, .3])

        def all_failed(report):
            report["frames"] = [{"input": "/assets/rolls/R/a.tif", "status": "failed",
                                 "error": "x"}]
        self.edit_report("p", all_failed)
        self.assertEqual(self.analyze("p")[0], 0)
        result = json.loads((self.root / "converted/nc/p/R/analysis.json").read_text())
        self.assertEqual(result["output_depth"], "u16")
        self.assertNotIn("output_depth", result["frames"][0])

    def test_explicit_output_path_and_clipping_fraction(self):
        self.write_run("b", [.2, .2, .1], clipped=5)
        path = self.root / "result.json"
        args = argparse.Namespace(asset_root=str(self.root), roll="R", run="b",
                                  out=str(path))
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(roll.cmd_analyze(args), 0)
        result = json.loads(path.read_text())
        self.assertEqual(result["frames"][0]["clip_fraction"], .05)


if __name__ == "__main__":
    unittest.main()
