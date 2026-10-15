"""Reference cells: pairing, colour-space resolution, conversion, and the
generator end to end over a fake `hanten` and synthetic exports."""
from __future__ import annotations

import argparse
import contextlib
import io
import json
import os
import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from nctool import metrics as _metrics  # noqa: E402
from nctool import reference  # noqa: E402
from nctool import review  # noqa: E402
from nctool.test_review import FAKE_NC, MATRIX, write  # noqa: E402

try:
    _metrics.require_dependencies()
    HAVE_DEPS = True
except _metrics.MetricsError:
    HAVE_DEPS = False
    if os.environ.get("NCTOOL_REQUIRE_DEPS"):
        raise

needs_deps = unittest.skipUnless(HAVE_DEPS, "needs the metrics venv")

DESTINATION = {"display": dict(reference.CELL_DESTINATION)}


def v2_profile(primaries: str = "adobe-rgb", gamma: float = 563 / 256) -> bytes:
    """A v2 matrix/TRC profile: D50-adapted colorants, a D65 `wtpt`, no `chad` —
    the shape SilverFast and Lightroom write."""
    m = _metrics
    to_d50 = m._mat_mul(m.bradford_adaptation("d65", "d50"),
                        m.rgb_to_xyz(primaries, "d65"))

    def xyz(values):
        return b"XYZ \0\0\0\0" + b"".join(struct.pack(">i", round(v * 65536))
                                          for v in values)

    curve = b"curv\0\0\0\0" + struct.pack(">IH", 1, round(gamma * 256)) + b"\0\0"
    tags = [(b"wtpt", xyz(m._xyz_of(m.WHITE["d65"])))]
    tags += [(name, xyz([to_d50[r][c] for r in range(3)]))
             for c, name in enumerate((b"rXYZ", b"gXYZ", b"bXYZ"))]
    tags += [(name, curve) for name in (b"rTRC", b"gTRC", b"bTRC")]
    offset = 128 + 4 + 12 * len(tags)
    table, data = b"", b""
    for name, body in tags:
        table += struct.pack(">4sII", name, offset + len(data), len(body))
        data += body + b"\0" * (-len(body) % 4)
    size = offset + len(data)
    header = bytearray(128)
    struct.pack_into(">I", header, 0, size)
    header[8] = 2
    header[12:24] = b"mntrRGB XYZ "
    header[36:40] = b"acsp"
    return bytes(header) + struct.pack(">I", len(tags)) + table + data


def write_export(path: Path, rgb=(0.5, 0.5, 0.5), size=(8, 6), profile=None,
                 preview: bool = False, extra_page: bool = False) -> None:
    import numpy as np
    import tifffile

    path.parent.mkdir(parents=True, exist_ok=True)
    width, height = size
    array = np.empty((height, width, 3), np.uint16)
    array[:] = [round(c * 65535) for c in rgb]
    tags = [(34675, 7, len(profile), profile, True)] if profile else []
    with tifffile.TiffWriter(str(path)) as tif:
        tif.write(array, photometric="rgb", extratags=tags)
        if preview or extra_page:
            tif.write(array[::2, ::2], photometric="rgb",
                      subfiletype=1 if preview else 0)


@needs_deps
class TestProfileSpace(unittest.TestCase):
    def test_reads_a_v2_adobe_rgb_profile(self):
        self.assertEqual(reference.profile_space(v2_profile()), "adobe-rgb")

    def test_reads_a_linear_profile_by_its_curves(self):
        self.assertEqual(reference.profile_space(v2_profile(gamma=1.0)),
                         "linear-adobe-rgb")
        self.assertEqual(reference.profile_space(v2_profile("rec709", 1.0)),
                         "linear-srgb")

    def test_reads_a_v4_srgb_profile_with_chad(self):
        from PIL import ImageCms
        raw = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
        self.assertEqual(reference.profile_space(raw), "srgb")

    def test_a_malformed_profile_is_a_reference_error(self):
        # Valid size and signature, one tag declared, no tag table: struct.error inside.
        header = bytearray(128)
        struct.pack_into(">I", header, 0, 132)
        header[36:40] = b"acsp"
        with self.assertRaisesRegex(reference.ReferenceError, "unreadable ICC profile"):
            reference.profile_space(bytes(header) + struct.pack(">I", 1))

    def test_names_what_an_unknown_profile_is(self):
        with self.assertRaisesRegex(reference.ReferenceError, "bt2020"):
            reference.profile_space(v2_profile("bt2020"))


@needs_deps
class TestResolveSpace(unittest.TestCase):
    def test_no_profile_needs_a_declaration(self):
        with self.assertRaisesRegex(reference.ReferenceError, "declare"):
            reference.resolve_space(None, None)
        self.assertEqual(reference.resolve_space(None, "adobe-rgb"),
                         ("adobe-rgb", "declared by the matrix"))

    def test_a_declaration_never_overrides_the_profile(self):
        with self.assertRaisesRegex(reference.ReferenceError, "declares srgb"):
            reference.resolve_space(v2_profile(), "srgb")
        self.assertEqual(reference.resolve_space(v2_profile(), "adobe-rgb")[1],
                         "embedded ICC profile")


@needs_deps
class TestConversion(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="nc-reference-"))

    def test_white_and_grey_map_through_and_saturated_green_clips(self):
        import numpy as np
        encoded = np.array([[[1.0, 1.0, 1.0], [0.0, 1.0, 0.0]]], np.float32)
        pixels, clipped, nonfinite = reference.to_srgb(encoded, "adobe-rgb")
        self.assertEqual(pixels[0, 0].tolist(), [255, 255, 255])
        self.assertEqual((clipped, nonfinite), (0.5, 0.0))
        grey = np.full((1, 1, 3), 0.5, np.float32)
        self.assertEqual(reference.to_srgb(grey, "srgb")[0][0, 0].tolist(),
                         [128, 128, 128])

    def test_non_finite_samples_are_counted_and_written_black(self):
        import numpy as np
        encoded = np.array([[[np.nan, 0.5, 0.5], [0.5, np.inf, 0.5],
                             [0.5, 0.5, 0.5], [0.5, 0.5, 0.5]]], np.float32)
        pixels, clipped, nonfinite = reference.to_srgb(encoded, "linear-srgb")
        self.assertEqual(nonfinite, 0.5)
        self.assertEqual(clipped, 0.0)
        self.assertEqual(pixels[0, :2].tolist(), [[0, 0, 0], [0, 0, 0]])

    def test_planar_and_non_rgb_tiffs_are_refused(self):
        import numpy as np
        import tifffile
        planar = np.zeros((3, 6, 8), np.uint16)
        tifffile.imwrite(self.dir / "planar.tif", planar, photometric="rgb",
                         planarconfig="separate")
        with self.assertRaisesRegex(reference.ReferenceError, "planar"):
            reference.read_export(self.dir / "planar.tif", pixels=False)
        tifffile.imwrite(self.dir / "grey.tif", np.zeros((6, 8, 3), np.uint16),
                         photometric="minisblack", planarconfig="contig")
        with self.assertRaisesRegex(reference.ReferenceError, "not RGB"):
            reference.read_export(self.dir / "grey.tif")

    def test_a_missing_source_fails_as_a_reference_error(self):
        with self.assertRaisesRegex(reference.ReferenceError, "FileNotFoundError"):
            reference.convert(self.dir / "gone.tif", self.dir / "gone.jpg", None, {}, None)

    def test_a_reduced_resolution_preview_is_accepted_a_second_image_is_not(self):
        write_export(self.dir / "p.tif", profile=v2_profile(), preview=True)
        self.assertIsNotNone(reference.read_export(self.dir / "p.tif")[1])
        write_export(self.dir / "x.tif", profile=v2_profile(), extra_page=True)
        with self.assertRaisesRegex(reference.ReferenceError, "full-resolution"):
            reference.read_export(self.dir / "x.tif")

    def test_a_conversion_is_reused_until_its_source_changes(self):
        source, jpeg = self.dir / "s.tif", self.dir / "s.jpg"
        write_export(source, profile=v2_profile())
        first = reference.convert(source, jpeg, None, {}, None)
        self.assertEqual((first["space"], first["width"], first["height"]),
                         ("adobe-rgb", 8, 6))
        stamp = jpeg.stat().st_mtime_ns
        self.assertIs(reference.convert(source, jpeg, None, first, None), first)
        self.assertEqual(jpeg.stat().st_mtime_ns, stamp)
        write_export(source, rgb=(0.2, 0.3, 0.4), profile=v2_profile())
        self.assertNotEqual(reference.convert(source, jpeg, None, first, None)
                            ["source_sha256"], first["source_sha256"])

    def test_the_written_jpeg_carries_an_srgb_profile(self):
        from PIL import Image
        source, jpeg = self.dir / "s.tif", self.dir / "s.jpg"
        write_export(source, profile=v2_profile())
        reference.convert(source, jpeg, None, {}, None)
        with Image.open(jpeg) as image:
            raw = image.info["icc_profile"]
        self.assertEqual(reference.profile_space(raw), "srgb")

    def test_a_file_that_drifted_from_the_manifest_is_refused(self):
        write_export(self.dir / "s.tif", profile=v2_profile())
        with self.assertRaisesRegex(reference.ReferenceError, "sha256"):
            reference.convert(self.dir / "s.tif", self.dir / "s.jpg", None, {}, "0" * 64)


class TestPairing(unittest.TestCase):
    MANIFEST = {"converted": {
        "sf/R": {"producer": "sf", "outputs": [
            {"file": "converted/sf/R/1.tif", "source_frame": "rolls/R/1.tif"},
            {"file": "converted/sf/R/9.tif", "source_frame": None}]},
        "nlp/R": {"producer": "nlp", "outputs": [
            {"file": "converted/nlp/R/1.tif", "source_frame": "rolls/R/1.tif"}]}}}

    def test_pairs_by_source_frame_within_one_producer(self):
        sf = reference.exports(self.MANIFEST, "sf")
        self.assertEqual([e["file"] for e in reference.pair(sf, "rolls/R/1.tif")],
                         ["converted/sf/R/1.tif"])
        self.assertEqual(reference.pair(sf, "rolls/R/2.tif"), [])

    def test_lists_exports_that_name_no_source_frame(self):
        sf = reference.exports(self.MANIFEST, "sf")
        self.assertEqual(reference.unpaired(sf, {"R"}), {"sf/R": ["9.tif"]})
        self.assertEqual(reference.unpaired(sf, {"other"}), {})


class TestMatrix(unittest.TestCase):
    BASE = {**{k: v for k, v in MATRIX.items() if k != "output_preset"},
            "destination": DESTINATION,
            "references": [{"id": "sf", "label": "SF", "producer": "silverfast"}]}

    def load(self, **overrides):
        return review.load_matrix(write({**self.BASE, **overrides}))

    def test_reads_references(self):
        [ref] = self.load()["references"]
        self.assertEqual((ref["id"], ref["producer"], ref["space"]),
                         ("sf", "silverfast", None))

    def test_hanten_cells_must_be_sdr_srgb_jpeg(self):
        hdr = {"display": {**reference.CELL_DESTINATION, "range": "hdr"}}
        with self.assertRaisesRegex(review.ReviewError, "SDR sRGB JPEG"):
            self.load(destination=hdr)
        with self.assertRaisesRegex(review.ReviewError, "remove output_preset"):
            self.load(output_preset="display-p3")

    def test_refuses_a_mistyped_key_an_unknown_space_and_a_reused_id(self):
        for refs, message in (
                ([{"id": "sf", "producer": "s", "spcae": "srgb"}], "unknown key"),
                ([{"id": "sf", "producer": "s", "space": "rec2020"}], "not one of"),
                ([{"id": "generic", "producer": "s"}], "reuse config id"),
                ([{"id": "a", "producer": "s"}, {"id": "a", "producer": "t"}],
                 "two entries")):
            with self.subTest(message=message):
                with self.assertRaisesRegex(review.ReviewError, message):
                    self.load(references=refs)

    def test_a_reference_is_checked_against_the_build_expanded_config_ids(self):
        builds = [{"id": "x", "nc": "/bin/x"}, {"id": "y", "nc": "/bin/y"}]
        matrix = self.load(builds=builds, references=[{"id": "generic", "producer": "s"}])
        self.assertEqual([c["id"] for c in matrix["configs"]][:2],
                         ["generic@x", "generic@y"])
        self.assertEqual(matrix["references"][0]["id"], "generic")


@needs_deps
class TestGenerate(unittest.TestCase):
    """`cmd_generate` with references, over a fake `hanten` and a synthetic manifest."""

    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="nc-reference-e2e-"))
        assets = self.root / "assets"
        for name in ("1", "2"):
            (assets / "rolls" / "R").mkdir(parents=True, exist_ok=True)
            (assets / "rolls" / "R" / f"{name}.tif").write_bytes(b"II*\x00")
        write_export(assets / "converted/sf/R/1.tif", profile=v2_profile())
        write_export(assets / "converted/sf/R/9.tif", profile=v2_profile())
        write_export(assets / "converted/sf/R/2.tif", size=(6, 6))
        self.manifest = {"schema_version": 4, "rolls": {"R": {"frames": [
            {"file": "rolls/R/1.tif", "width": 8, "height": 6},
            {"file": "rolls/R/2.tif", "width": 8, "height": 6}]}},
            "converted": {"sf/R": {"producer": "sf", "outputs": [
                {"file": "converted/sf/R/1.tif", "source_frame": "rolls/R/1.tif"},
                {"file": "converted/sf/R/2.tif", "source_frame": "rolls/R/2.tif"},
                {"file": "converted/sf/R/9.tif", "source_frame": None}]}}}
        self.write_manifest()
        (self.root / "fixtures.json").write_text(json.dumps(
            {"rolls": {"R": {"dmin": [0.9, 0.9, 0.9]}},
             "frames": {"F1": {"roll": "R", "file": "1.tif"},
                        "F2": {"roll": "R", "file": "2.tif"}}}), encoding="utf-8")
        self.nc = self.root / "fake"
        self.nc.write_text(FAKE_NC.format(python=sys.executable, commits=["aaa"],
                                          banner="aaa", version=8, reports=8,
                                          name="fake"), encoding="utf-8")
        self.nc.chmod(0o755)

    def write_manifest(self):
        (self.root / "assets" / "manifest.json").write_text(json.dumps(self.manifest),
                                                            encoding="utf-8")

    def run_generate(self, references=None, version=8, no_metrics=True,
                     **matrix) -> tuple[int, str]:
        if version != 8:
            self.nc.write_text(self.nc.read_text().replace("pipeline_version: 8",
                                                           f"pipeline_version: {version}"))
        path = write({**TestMatrix.BASE, "common_args": [], "metrics": {"inset": 0.0},
                      "configs": [{"id": "dflt", "args": []}],
                      "references": references or [{"id": "sf", "producer": "sf",
                                                     "space": "adobe-rgb"}],
                      **matrix})
        args = argparse.Namespace(
            matrix=str(path), fixtures=str(self.root / "fixtures.json"), frames=None,
            nc=str(self.nc), build=None, asset_root=str(self.root / "assets"),
            out=str(self.root / "out"), no_metrics=no_metrics, force=False)
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = review.cmd_generate(args)
        return code, err.getvalue()

    def review_json(self) -> dict:
        return json.loads((self.root / "out" / "review.json").read_text())

    def test_a_reference_cell_sits_beside_the_render_with_its_space_recorded(self):
        code, err = self.run_generate()
        self.assertEqual(code, 0, err)
        review_json = self.review_json()
        self.assertEqual([c["id"] for c in review_json["configs"]], ["dflt", "sf"])
        producer = review_json["configs"][1]["producer"]
        self.assertEqual((producer["kind"], producer["label"]), ("external", "sf"))
        self.assertIn("adobe-rgb (embedded ICC profile) x1", producer["note"])
        self.assertIn("adobe-rgb (declared by the matrix) x1", producer["note"])
        f1 = review_json["images"][0]
        self.assertEqual(f1["renditions"]["sf"],
                         {"src": "F1-sf.jpg", "width": 8, "height": 6})
        record = json.loads((self.root / "out" / "F1-sf.reference.json").read_text())
        self.assertEqual((record["source"], record["space"]),
                         ("converted/sf/R/1.tif", "adobe-rgb"))

    def test_unpaired_exports_and_frames_without_one_are_reported(self):
        self.manifest["converted"]["sf/R"]["outputs"].pop(1)
        self.write_manifest()
        code, err = self.run_generate()
        self.assertEqual(code, 0, err)
        self.assertIn("1 export(s) in sf/R name no source_frame", err)
        self.assertIn("9.tif", err)
        self.assertIn("F2/sf: no sf export of this frame", err)
        self.assertIn("FAILED cells (1): F2/sf", err)
        self.assertIn("sf: no export", self.review_json()["images"][1]["note"])

    def test_a_cropped_export_gets_no_charts_and_says_why(self):
        code, err = self.run_generate()
        self.assertEqual(code, 0, err)
        f2 = self.review_json()["images"][1]
        self.assertNotIn("metrics", f2["renditions"]["sf"])
        self.assertIn("sf: cropped to 6x6, no charts", f2["note"])

    def test_a_profileless_export_without_a_declared_space_fails_its_cell(self):
        code, err = self.run_generate(references=[{"id": "sf", "producer": "sf"}])
        self.assertEqual(code, 0, err)
        self.assertIn("F2/sf: converted/sf/R/2.tif: the file has no ICC profile", err)
        self.assertNotIn("sf", self.review_json()["images"][1]["renditions"])

    def test_an_export_with_the_scans_dimensions_is_charted_in_srgb(self):
        code, err = self.run_generate(no_metrics=False)
        self.assertEqual(code, 0, err)
        f1, f2 = self.review_json()["images"]
        self.assertEqual(f1["renditions"]["sf"]["metrics"], "F1-sf.jpg.metrics.json")
        record = json.loads((self.root / "out" / "F1-sf.jpg.metrics.json").read_text())
        self.assertEqual(record["space"]["declared"], "srgb")
        self.assertNotIn("metrics", f2["renditions"]["sf"])
        self.assertFalse((self.root / "out" / "F2-sf.jpg.metrics.json").exists())

    def test_a_missing_export_file_fails_only_its_cell(self):
        (self.root / "assets" / "converted/sf/R/1.tif").unlink()
        code, err = self.run_generate()
        self.assertEqual(code, 0, err)
        self.assertIn("F1/sf: converted/sf/R/1.tif: FileNotFoundError", err)
        self.assertIn("FAILED cells (1): F1/sf", err)
        f1, f2 = self.review_json()["images"]
        self.assertNotIn("sf", f1["renditions"])
        self.assertIn("sf: not converted", f1["note"])
        self.assertIn("sf", f2["renditions"])

    def test_two_exports_of_one_frame_fail_the_cell_rather_than_guess(self):
        self.manifest["converted"]["sf/R"]["outputs"][2]["source_frame"] = "rolls/R/1.tif"
        self.write_manifest()
        code, err = self.run_generate()
        self.assertEqual(code, 0, err)
        self.assertIn("F1/sf: 2 sf exports of this frame", err)
        f1 = self.review_json()["images"][0]
        self.assertNotIn("sf", f1["renditions"])
        self.assertIn("sf: 2 exports, not paired", f1["note"])

    def test_an_export_that_drifted_from_the_manifest_fails_its_cell(self):
        self.manifest["converted"]["sf/R"]["outputs"][0]["sha256"] = "0" * 64
        self.write_manifest()
        code, err = self.run_generate()
        self.assertEqual(code, 0, err)
        self.assertIn("F1/sf: converted/sf/R/1.tif: the file no longer matches", err)
        self.assertNotIn("sf", self.review_json()["images"][0]["renditions"])
        self.assertIn("sf", self.review_json()["images"][1]["renditions"])

    def test_a_producer_the_manifest_lacks_is_refused_before_rendering(self):
        code, err = self.run_generate(references=[{"id": "n", "producer": "nlp"}])
        self.assertEqual(code, 2)
        self.assertIn("no converted/nlp/ exports; its producers are sf", err)
        self.assertFalse((self.root / "out" / "review.json").exists())
        self.assertFalse((self.root / "fake.argv").exists())

    def test_a_rerun_converts_nothing(self):
        self.assertEqual(self.run_generate()[0], 0)
        jpeg = self.root / "out" / "F1-sf.jpg"
        stamp = jpeg.stat().st_mtime_ns
        self.assertEqual(self.run_generate()[0], 0)
        self.assertEqual(jpeg.stat().st_mtime_ns, stamp)

    def test_a_preset_build_is_refused_with_references(self):
        code, err = self.run_generate(version=5)
        self.assertEqual(code, 2)
        self.assertIn("takes output presets and has no SDR sRGB JPEG", err)
        self.assertNotIn("states no output_preset", err)


if __name__ == "__main__":
    unittest.main()
