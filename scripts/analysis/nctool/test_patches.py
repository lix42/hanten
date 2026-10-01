"""Stdlib unit tests for `nctool.patches` (run: `python3 -m unittest`)."""
from __future__ import annotations

import argparse
import contextlib
import io
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from nctool import patches  # noqa: E402

COPY_ALL = """# Review notes — a set

1 of 2 frames noted · 3 patches on 2 frames

## Roll · 1981

soft highlights, ignore this line

Patches:
- "White pillow" — x 52.5% y 31.4% w 7.0% h 5.0%
- "a \\"quoted\\" sheet" — x 6.8% y 76.1% w 6.6% h 4.4%

## F2

Patches:
- "cloud" — x 12.0% y 37.9% w 4.5% h 4.1%
"""

GOOD = {"label": "cloud", "rect": [0.1, 0.2, 0.3, 0.4], "kind": "white", "source": "s"}


class TestCheck(unittest.TestCase):
    def test_well_formed(self):
        self.assertEqual(patches.check_patch(GOOD), [])
        self.assertEqual(patches.check_patch(dict(GOOD, light="shade")), [])

    def test_each_rule(self):
        cases = [
            (dict(GOOD, rect=[0.9, 0, 0.2, 0.1]), "inside the frame"),
            (dict(GOOD, rect=[0.5, 0, 0.502, 0.1]), "inside the frame"),
            (dict(GOOD, rect=[0.1, 0.1, 0, 0.1]), "inside the frame"),
            (dict(GOOD, rect=[0.1, 0.1, 0.1]), "four numbers"),
            (dict(GOOD, rect=[True, 0, 0.1, 0.1]), "four numbers"),
            (dict(GOOD, kind="neutral"), "kind 'neutral'"),
            (dict(GOOD, light="dusk"), "light 'dusk'"),
            (dict(GOOD, label=" "), "label"),
            ({k: v for k, v in GOOD.items() if k != "source"}, "source"),
            (dict(GOOD, colour="red"), "unknown key 'colour'"),
        ]
        for p, needle in cases:
            errs = patches.check_patch(p)
            self.assertTrue(any(needle in e for e in errs), (p, errs))


    def test_rounding_at_the_edge_is_tolerated_and_clamped(self):
        # The app clamps x + w to 1; Copy all's 0.1 % rounding prints 100.1 %.
        self.assertEqual(patches.check_patch(dict(GOOD, rect=[0.001, 0, 1.0, 0.5])), [])
        self.assertEqual(patches.clamp_rect([0.001, 0.2, 1.0, 0.801]), [0.001, 0.2, 0.999, 0.8])

    def test_frame_patches_must_be_a_list(self):
        self.assertEqual(patches.check_frame_patches({"file": "f"}), [])
        self.assertEqual(patches.check_frame_patches({"patches": None}),
                         ["patches is not a list (NoneType)"])
        self.assertEqual(patches.check_frame_patches({"patches": [dict(GOOD, kind="x")]}),
                         ["patch 0: kind 'x' is not one of white, grey, colour, unknown"])

    def test_label_prefix_sets_the_kind(self):
        self.assertEqual(patches.split_kind("W: cloth", "unknown"), ("cloth", "white"))
        self.assertEqual(patches.split_kind("C: sea", "white"), ("sea", "colour"))
        self.assertEqual(patches.split_kind("G", "white"), ("grey", "grey"))
        self.assertEqual(patches.split_kind("cloud", "white"), ("cloud", "white"))
        self.assertEqual(patches.split_kind("Wall: east", "white"), ("Wall: east", "white"))


class TestParse(unittest.TestCase):
    def test_copy_all(self):
        got = patches.parse_copy_all(COPY_ALL)
        self.assertEqual(list(got), ["Roll · 1981", "F2"])
        self.assertEqual(got["Roll · 1981"][0], ("White pillow", [0.525, 0.314, 0.07, 0.05]))
        self.assertEqual(got["Roll · 1981"][1][0], 'a "quoted" sheet')
        self.assertEqual(got["F2"], [("cloud", [0.12, 0.379, 0.045, 0.041])])

    def test_an_unparsed_patch_line_is_an_error(self):
        bad = COPY_ALL.replace('- "cloud" — x 12.0%', '- "cloud" — x 12.0% kind white')
        with self.assertRaisesRegex(ValueError, "line 16 is not a patch"):
            patches.parse_copy_all(bad)

    def test_list_items_in_a_note_are_not_patches(self):
        text = "## F2\n\n- a bullet in a note\n\nPatches:\n" \
               '- "cloud" — x 1.0% y 1.0% w 1.0% h 1.0%\n'
        self.assertEqual(list(patches.parse_copy_all(text)), ["F2"])

    def test_duplicate_headings_are_refused(self):
        with self.assertRaisesRegex(ValueError, "two images headed 'x'"):
            patches.image_ids_by_heading({"images": [{"id": "a", "label": "x"},
                                                     {"id": "b", "label": "x"}]})

    def test_resolve_frame_across_rolls(self):
        rolls = {"A": {"frames": [{"file": "rolls/A/1981.tif"}]},
                 "B": {"frames": [{"file": "rolls/B/1981.tif"}, {"file": "rolls/B/7.tif"}]}}
        self.assertEqual(patches.resolve_frame(rolls, "x-7", None)[0]["file"], "rolls/B/7.tif")
        self.assertEqual(patches.resolve_frame(rolls, "1981", "A")[0]["file"], "rolls/A/1981.tif")
        fr, why = patches.resolve_frame(rolls, "1981", None)
        self.assertIsNone(fr)
        self.assertIn("several rolls (A, B)", why)
        self.assertIn("no frame of A", patches.resolve_frame(rolls, "7", "A")[1])

    def test_frame_for_image_by_stem_or_serial(self):
        frames = [{"file": "rolls/R/1981.tif"}, {"file": "rolls/R/base.tif"}]
        self.assertIs(patches.frame_for_image(frames, "1981"), frames[0])
        self.assertIs(patches.frame_for_image(frames, "p400-0928d-1981"), frames[0])
        self.assertIsNone(patches.frame_for_image(frames, "1982"))

    def test_replace_source_keeps_other_sources(self):
        old = [dict(GOOD, source="a"), dict(GOOD, source="b")]
        new = [dict(GOOD, source="b", label="new")]
        self.assertEqual(patches.replace_source(old, new, "b"), [old[0], new[0]])


class TestImport(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.A = tmp.name
        self.manifest = os.path.join(self.A, "manifest.json")
        frames = [{"file": f"rolls/R/{s}.tif", "role": "real"} for s in ("1981", "1982")]
        frames[0]["patches"] = [dict(GOOD, source="old"), dict(GOOD, source="this")]
        with open(self.manifest, "w") as f:
            json.dump({"schema_version": 1, "rolls": {"R": {"frames": frames}}}, f)
        self.review = os.path.join(self.A, "review.json")
        with open(self.review, "w") as f:
            json.dump({"schema_version": 1, "images": [
                {"id": "1981", "label": "Roll · 1981"}, {"id": "r-1982"}]}, f)
        self.notes = os.path.join(self.A, "notes.md")

    def run_import(self, text, **kw):
        with open(self.notes, "w") as f:
            f.write(text)
        args = argparse.Namespace(asset_root=self.A, notes=self.notes, review=self.review,
                                  roll="R", source="this", kind="white", dry_run=False)
        for k, v in kw.items():
            setattr(args, k, v)
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = patches.cmd_import(args)
        with open(self.manifest) as f:
            return rc, json.load(f), err.getvalue()

    def test_imports_and_replaces_only_the_same_source(self):
        rc, m, err = self.run_import(COPY_ALL.replace("## F2", "## r-1982"))
        self.assertEqual(rc, 0, err)
        self.assertEqual(m["schema_version"], 2)
        f1, f2 = m["rolls"]["R"]["frames"]
        self.assertEqual([p["source"] for p in f1["patches"]], ["old", "this", "this"])
        self.assertEqual(f1["patches"][1]["label"], "White pillow")
        self.assertEqual(f2["patches"], [{"label": "cloud", "rect": [0.12, 0.379, 0.045, 0.041],
                                          "kind": "white", "source": "this"}])

    def test_two_headings_naming_one_frame_add_up(self):
        with open(self.review, "w") as f:
            json.dump({"schema_version": 1, "images": [
                {"id": "1981", "label": "Roll · 1981"}, {"id": "p-1981", "label": "F2"}]}, f)
        rc, m, err = self.run_import(COPY_ALL)
        self.assertEqual(rc, 0, err)
        f1 = m["rolls"]["R"]["frames"][0]
        self.assertEqual([p["label"] for p in f1["patches"] if p["source"] == "this"],
                         ["White pillow", 'a "quoted" sheet', "cloud"])

    def test_label_prefixes_set_kinds_and_the_flag_covers_the_rest(self):
        text = COPY_ALL.replace('"cloud"', '"C: red flag"').replace("## F2", "## r-1982")
        rc, m, err = self.run_import(text)
        self.assertEqual(rc, 0, err)
        p = m["rolls"]["R"]["frames"][1]["patches"][0]
        self.assertEqual((p["label"], p["kind"]), ("red flag", "colour"))
        self.assertEqual(m["rolls"]["R"]["frames"][0]["patches"][1]["kind"], "white")

    def test_without_roll_frames_resolve_across_rolls(self):
        with open(self.manifest) as f:
            data = json.load(f)
        data["rolls"]["S"] = {"frames": [{"file": "rolls/S/1990.tif", "role": "real"}]}
        with open(self.manifest, "w") as f:
            json.dump(data, f)
        with open(self.review, "w") as f:
            json.dump({"schema_version": 1, "images": [
                {"id": "1981", "label": "Roll · 1981"}, {"id": "s-1990", "label": "F2"}]}, f)
        rc, m, err = self.run_import(COPY_ALL, roll=None)
        self.assertEqual(rc, 0, err)
        self.assertEqual(m["rolls"]["S"]["frames"][0]["patches"][0]["label"], "cloud")

    def test_an_edge_patch_is_clamped_not_refused(self):
        text = COPY_ALL.replace("x 12.0% y 37.9% w 4.5%", "x 0.1% y 37.9% w 100.0%")
        rc, m, err = self.run_import(text.replace("## F2", "## r-1982"))
        self.assertEqual(rc, 0, err)
        self.assertEqual(m["rolls"]["R"]["frames"][1]["patches"][0]["rect"][:3],
                         [0.001, 0.379, 0.999])

    def test_missing_files_and_bad_patch_lists_exit_2(self):
        rc, _, err = self.run_import(COPY_ALL, review=os.path.join(self.A, "nope.json"))
        self.assertEqual(rc, 2)
        self.assertIn("nope.json", err)
        with open(self.manifest) as f:
            data = json.load(f)
        data["rolls"]["R"]["frames"][0]["patches"] = None
        with open(self.manifest, "w") as f:
            json.dump(data, f)
        rc, _, err = self.run_import(COPY_ALL.replace("## F2", "## r-1982"))
        self.assertEqual(rc, 2)
        self.assertIn("not a list", err)

    def test_an_unmatched_heading_writes_nothing(self):
        rc, m, err = self.run_import(COPY_ALL)  # "F2" is no image in review.json
        self.assertEqual(rc, 2)
        self.assertIn("'F2' is no image in review.json", err)
        self.assertEqual(m["schema_version"], 1)
        self.assertEqual(len(m["rolls"]["R"]["frames"][0]["patches"]), 2)

    def test_text_without_patches_is_refused(self):
        rc, _, err = self.run_import("# Review notes\n\n## Roll · 1981\n\njust a note\n")
        self.assertEqual(rc, 2)
        self.assertIn("no patches found", err)

    def test_dry_run_writes_nothing(self):
        rc, m, _ = self.run_import(COPY_ALL.replace("## F2", "## r-1982"), dry_run=True)
        self.assertEqual(rc, 0)
        self.assertEqual(m["schema_version"], 1)


if __name__ == "__main__":
    unittest.main()
