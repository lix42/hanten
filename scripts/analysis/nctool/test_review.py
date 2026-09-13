"""Hermetic tests for the review-set generator.

Nothing here renders or measures: rendering needs the nc binary and the user's
own scans, and measuring needs the numpy venv. What is covered is everything that
decides *what* gets rendered and whether a stored measurement can be reused —
the parts a wrong answer in makes a whole page quietly wrong.
"""
from __future__ import annotations

import argparse
import contextlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from nctool import metrics as _metrics  # noqa: E402
from nctool import review  # noqa: E402

MATRIX = {
    "schema_version": 1,
    "title": "Two presets",
    "output_preset": "gain-map-hdr",
    "common_args": ["--film-base", "{dmin}"],
    "rolls": {"Ektar": {"film_stock": "ektar-100"}},
    "metrics": {"inset": 0.1},
    "configs": [
        {"id": "generic", "label": "generic", "args": ["--preset", "characteristic-generic"]},
        {"id": "stock", "label": "stock",
         "args": ["--preset", "characteristic-stock", "--film-stock", "{film_stock}"]},
    ],
}


def write(matrix: dict) -> Path:
    directory = Path(tempfile.mkdtemp(prefix="nc-review-matrix-"))
    path = directory / "matrix.json"
    path.write_text(json.dumps(matrix), encoding="utf-8")
    return path


def load(**overrides) -> dict:
    return review.load_matrix(write({**MATRIX, **overrides}))


class TestMatrix(unittest.TestCase):
    def test_reads_the_configs_in_order(self):
        matrix = load()
        self.assertEqual([c["id"] for c in matrix["configs"]], ["generic", "stock"])
        self.assertEqual(matrix["suffix"], "jpg")

    def test_refuses_an_unknown_output_preset(self):
        with self.assertRaisesRegex(review.ReviewError, "unknown output_preset"):
            load(output_preset="gain-map-hrd")

    def test_refuses_a_future_schema(self):
        with self.assertRaisesRegex(review.ReviewError, "schema_version"):
            load(schema_version=2)

    def test_refuses_two_configs_with_one_id(self):
        with self.assertRaisesRegex(review.ReviewError, "two entries with id"):
            load(configs=[{"id": "a", "args": []}, {"id": "a", "args": []}])

    def test_refuses_an_empty_matrix(self):
        with self.assertRaisesRegex(review.ReviewError, "at least one"):
            load(configs=[])

    # A typo in a placeholder is the failure this catches at the top rather than
    # 35 renders later — and `--film-stock {film_stok}` would otherwise reach nc
    # as a literal stock name.
    def test_refuses_an_unknown_placeholder(self):
        with self.assertRaisesRegex(review.ReviewError, r"unknown placeholder \{film_stok\}"):
            load(configs=[{"id": "a", "args": ["--film-stock", "{film_stok}"]}])

    def test_refuses_an_unterminated_placeholder(self):
        with self.assertRaisesRegex(review.ReviewError, "unterminated placeholder"):
            load(configs=[{"id": "a", "args": ["{dmin"]}])

    def test_refuses_an_inset_that_would_measure_nothing(self):
        with self.assertRaisesRegex(review.ReviewError, "metrics.inset"):
            load(metrics={"inset": 0.5})

    # Which configs need a film stock is **stated by the args**, never guessed
    # from the id: nc refuses `--film-stock` beside a preset with no stock to
    # configure, and requires it for the ones that have one.
    def test_only_the_configs_that_name_a_stock_need_one(self):
        configs = {c["id"]: c for c in load()["configs"]}
        self.assertEqual(configs["generic"]["needs"], {"dmin"})
        self.assertEqual(configs["stock"]["needs"], {"dmin", "film_stock"})


class TestUnknownKeys(unittest.TestCase):
    """`deny_unknown_fields`, as every recipe struct in this repo has."""

    # `arg` for `args` loads as *no* arguments, so the cell renders the default
    # conversion under a label promising something else: five buttons, five
    # labels, identical pixels, exit 0.
    def test_refuses_a_mistyped_config_key(self):
        with self.assertRaisesRegex(review.ReviewError, "unknown key arg;"):
            load(configs=[{"id": "a", "arg": ["--preset", "sigmoid-flat"]}])

    # `insets` measures the whole frame — the film holder included, which is the
    # one thing the inset exists to keep out of the statistics.
    def test_refuses_a_mistyped_metrics_key(self):
        with self.assertRaisesRegex(review.ReviewError, "unknown key insets;"):
            load(metrics={"insets": 0.18})

    def test_refuses_a_mistyped_top_level_key(self):
        with self.assertRaisesRegex(review.ReviewError, "unknown key output_presets;"):
            load(output_presets="gain-map-hdr")

    def test_refuses_a_mistyped_roll_key(self):
        with self.assertRaisesRegex(review.ReviewError, "unknown key stock;"):
            load(rolls={"Ektar": {"stock": "ektar-100"}})


class TestCopiedStrings(unittest.TestCase):
    """Copied into `review.json`, where the app refuses a non-string."""

    def test_refuses_a_title_that_is_not_a_string(self):
        with self.assertRaisesRegex(review.ReviewError, "title must be a non-empty string"):
            load(title=2026)

    def test_refuses_a_note_that_is_not_a_string(self):
        with self.assertRaisesRegex(review.ReviewError, r"configs\[0\].note"):
            load(configs=[{"id": "a", "note": 123}])


class TestExpansion(unittest.TestCase):
    def test_substitutes_the_per_frame_values(self):
        self.assertEqual(
            review.expand_args(["--film-base", "{dmin}", "--film-stock", "{film_stock}"],
                               {"dmin": "0.5,0.2,0.1", "film_stock": "ektar-100"}),
            ["--film-base", "0.5,0.2,0.1", "--film-stock", "ektar-100"])

    def test_leaves_an_argument_with_no_placeholder_alone(self):
        self.assertEqual(review.expand_args(["--preset", "sigmoid-flat"], {"dmin": "x"}),
                         ["--preset", "sigmoid-flat"])


class TestMetricsSpace(unittest.TestCase):
    def test_reads_the_gain_map_pair_as_its_sdr_base(self):
        space, _ = review.metrics_space("gain-map-hdr")
        self.assertEqual(space, "display-p3")

    # The preset name does not determine the space. `legacy` and `custom` accept
    # `--output-profile`, which a matrix is free to pass, and measuring ProPhoto
    # pixels as sRGB makes every tone and cast number wrong while every one of
    # them still looks reasonable.
    def test_a_cell_is_measured_in_the_space_its_own_recipe_resolved(self):
        space, _ = review.cell_space(
            {"output": {"preset": "legacy", "output_profile": "prophoto"}}, "legacy")
        self.assertEqual(space, "prophoto-gamma1.8")
        self.assertEqual(review.cell_space({"output": {"preset": "legacy"}}, "legacy")[0], "srgb")

    def test_a_cell_whose_space_is_under_determined_is_not_measured(self):
        space, why = review.cell_space({"output": {"preset": "custom", "depth": "f32"}}, "custom")
        self.assertIsNone(space)
        self.assertIn("f32", why)

    # `space_for_recipe` defaults an unstated preset to `gain-map-hdr`, so a
    # report that could not be read would resolve Display P3 for a `film-master`
    # render — linear ACEScg pixels measured as an encoded display space.
    def test_a_render_that_does_not_identify_itself_is_not_measured(self):
        for recipe in ({}, {"output": {}}, {"output": {"preset": "gain-map-hdr"}}):
            space, why = review.cell_space(recipe, "film-master")
            self.assertIsNone(space, recipe)
            self.assertIn("film-master", why)

    # A preset this toolkit cannot read is still perfectly reviewable by eye, so
    # the run produces a page without charts rather than refusing.
    def test_reports_why_an_unreadable_preset_has_no_metrics(self):
        space, why = review.metrics_space("hdr-pq")
        self.assertIsNone(space)
        self.assertIn("AVIF", why)

    def test_the_suffix_table_covers_every_preset_the_metrics_know(self):
        # The two tables are keyed by the same preset names; one gaining a preset
        # the other has never heard of is how a run renders `.tiff` from an AVIF
        # preset, or measures a container it cannot read.
        known = set(_metrics.PRESET_SPACES) | set(_metrics.PRESET_UNREADABLE)
        self.assertEqual(known - set(review.PRESET_SUFFIX), set())


class TestOwnedFlags(unittest.TestCase):
    """`nc` takes the last occurrence of a Set argument, so an override is silent."""

    def test_refuses_a_config_restating_the_output_preset(self):
        with self.assertRaisesRegex(review.ReviewError, "state it once as output_preset"):
            load(configs=[{"id": "a", "args": ["--output-preset", "film-master"]}])

    def test_refuses_the_flags_the_generator_supplies(self):
        for flag in ("-o", "--output", "--report"):
            with self.assertRaisesRegex(review.ReviewError, "set by the generator"):
                load(common_args=[flag, "x"])

    def test_sees_the_equals_form_too(self):
        with self.assertRaisesRegex(review.ReviewError, "set by the generator"):
            load(configs=[{"id": "a", "args": ["--output-preset=legacy"]}])


class TestDimensions(unittest.TestCase):
    """`nc`'s report carries no image size, so the record is the only source."""

    def test_states_both_dimensions_or_neither(self):
        self.assertEqual(review._dimensions({"image": {"width": 5184, "height": 3600}}),
                         {"width": 5184, "height": 3600})
        for absent in ({}, {"image": {}}, {"image": {"width": 5184}},
                       {"image": {"width": 0, "height": 3600}}):
            self.assertEqual(review._dimensions(absent), {}, absent)


class TestOutputDirectory(unittest.TestCase):
    def test_refuses_to_render_into_the_repository(self):
        # The frames are the user's own photographs and are never committed.
        repo = Path(review.__file__).resolve().parents[3]
        for target in (repo, repo / "tools" / "review-app" / "public"):
            args = argparse.Namespace(
                matrix=str(write(MATRIX)), fixtures="scripts/sigmoid-baseline/fixtures.json",
                frames=None, nc="target/release/nc", asset_root="../nc-assets",
                out=str(target), no_metrics=True, force=False)
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                self.assertEqual(review.cmd_generate(args), 2)
            self.assertIn("inside the repository", err.getvalue())


class TestReuse(unittest.TestCase):
    RECORD = {"schema_version": _metrics.SCHEMA, "sha256": "abc",
              "region": {"x": 10, "y": 10, "width": 80, "height": 60}}

    def test_reuses_a_record_describing_these_exact_bytes(self):
        self.assertTrue(review.is_measured(self.RECORD, "abc", self.RECORD["region"]))

    def test_re_measures_when_the_render_changed(self):
        self.assertFalse(review.is_measured(self.RECORD, "def", self.RECORD["region"]))

    # The region is part of the identity: the same file measured over a different
    # rectangle is a different measurement, and reusing one for the other would
    # silently chart the holder.
    def test_re_measures_when_the_region_moved(self):
        moved = {**self.RECORD["region"], "x": 0}
        self.assertFalse(review.is_measured(self.RECORD, "abc", moved))

    def test_re_measures_a_record_from_an_older_schema(self):
        self.assertFalse(
            review.is_measured({**self.RECORD, "schema_version": _metrics.SCHEMA - 1},
                               "abc", self.RECORD["region"]))

    def test_re_measures_when_there_is_no_record_at_all(self):
        self.assertFalse(review.is_measured({}, "abc", self.RECORD["region"]))


class TestReviewDocument(unittest.TestCase):
    IMAGES = [{"id": "E1", "label": "E1", "renditions": {"generic": {"src": "E1-generic.jpg"}}}]

    def test_emits_the_schema_the_app_parses(self):
        doc = review.build_review(load(), self.IMAGES)
        self.assertEqual(doc["schema_version"], review.REVIEW_SCHEMA)
        self.assertEqual([c["id"] for c in doc["configs"]], ["generic", "stock"])
        self.assertEqual(doc["title"], "Two presets")

    # `configs` order is the button order **and** the keyboard mapping in the
    # app, so it follows the matrix rather than whatever rendered first.
    def test_declares_every_config_even_when_none_of_its_cells_rendered(self):
        doc = review.build_review(load(), self.IMAGES)
        self.assertIn("stock", [c["id"] for c in doc["configs"]])
        self.assertNotIn("stock", doc["images"][0]["renditions"])

    def test_leaves_out_an_absent_description(self):
        self.assertNotIn("description", review.build_review(load(), self.IMAGES))


class TestFrameSelection(unittest.TestCase):
    FIXTURES = {"frames": {"E1": {}, "E2": {}, "G1": {}}}

    def test_renders_every_fixture_frame_when_neither_states_any(self):
        self.assertEqual(review._frames_to_render(load(), self.FIXTURES, None),
                         ["E1", "E2", "G1"])

    def test_the_matrix_can_name_a_subset(self):
        self.assertEqual(review._frames_to_render(load(frames=["G1"]), self.FIXTURES, None),
                         ["G1"])

    def test_the_command_line_wins_over_the_matrix(self):
        self.assertEqual(review._frames_to_render(load(frames=["G1"]), self.FIXTURES, "E1,E2"),
                         ["E1", "E2"])

    def test_refuses_a_frame_the_fixtures_do_not_declare(self):
        with self.assertRaisesRegex(review.ReviewError, "no such frame"):
            review._frames_to_render(load(), self.FIXTURES, "E1,Z9")


class TestShippedMatrix(unittest.TestCase):
    """The committed matrix is data the generator reads, so it is checked here."""

    PATH = (Path(__file__).resolve().parents[2]
            / "preset-review" / "presets.matrix.json")

    def test_the_preset_matrix_loads(self):
        matrix = review.load_matrix(self.PATH)
        self.assertEqual(len(matrix["configs"]), 5)
        self.assertEqual(matrix["output_preset"], "gain-map-hdr")

    def test_every_roll_it_names_states_a_film_stock(self):
        matrix = review.load_matrix(self.PATH)
        for name, roll in matrix["rolls"].items():
            self.assertTrue(roll["film_stock"], f"{name} states no film stock")

    def test_its_frames_and_rolls_exist_in_the_fixtures(self):
        # The fixture declaration is the frame source, so a roll named only in
        # the matrix would silently lose every cell that needs its stock.
        fixtures = json.loads(
            (self.PATH.parents[1] / "sigmoid-baseline" / "fixtures.json")
            .read_text(encoding="utf-8"))
        matrix = review.load_matrix(self.PATH)
        self.assertEqual(set(matrix["rolls"]) - set(fixtures["rolls"]), set())
        for name in review._frames_to_render(matrix, fixtures, None):
            self.assertIn(fixtures["frames"][name]["roll"], fixtures["rolls"])


if __name__ == "__main__":
    unittest.main()
