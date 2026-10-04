"""Tests for `nctool viewer`: the file set's config, cases and rubric, the image-size
readers, and `check`'s verdicts against fake decoders (the real ones are macOS-only
and not on CI). One end-to-end case renders the chart through the debug binary."""

from __future__ import annotations

import copy
import json
import math
import os
import shutil
import struct
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from nctool import __main__ as cli  # noqa: E402
from nctool import acceptance, compare, viewer  # noqa: E402

NC = os.path.join(compare.repo_root(), "target", "debug", "hanten")
# What the committed config must yield: one destination per ready ROWS row, plus the
# film master (`default` and `direct` state no destination flag).
DESTINATIONS = {
    "sdr-native-display-p3-tiff", "sdr-native-adobe-rgb-tiff", "sdr-native-srgb-tiff",
    "hdr-linear-display-p3-tiff", "hdr-linear-adobe-rgb-tiff", "hdr-linear-srgb-tiff",
    "hdr-linear-bt2020-tiff", "hdr-pq-bt2020-tiff", "hdr-hlg-bt2020-tiff",
    "hdr-native-display-p3-jpeg", "hdr-native-srgb-jpeg", "film-master",
}


def _config() -> dict:
    return viewer.load_config(viewer.CONFIG)


def _write_config(tmp: str, cfg: dict) -> str:
    path = os.path.join(tmp, "viewer.json")
    with open(path, "w") as f:
        json.dump(cfg, f)
    return path


class Config(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="nc-viewer-test-")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def refused(self, cfg: dict, wording: str):
        with self.assertRaises(viewer.ViewerError) as e:
            viewer.load_config(_write_config(self.tmp, cfg))
        self.assertIn(wording, str(e.exception))

    def test_the_committed_config_loads_and_covers_every_encoding(self):
        cfg = _config()
        self.assertEqual(set(cfg["expect"]), set(acceptance.ENCODINGS))

    def test_only_the_gain_map_gates_its_fallback(self):
        gated = {k for k, e in _config()["expect"].items() if e["fallback_gated"]}
        self.assertEqual(gated, {"gain-map-jpeg"})

    def test_tiffs_go_to_apple_readers_only(self):
        for enc, e in _config()["expect"].items():
            if enc != "gain-map-jpeg":
                self.assertNotIn("chrome", e["readers"], enc)
                self.assertNotIn("sdr-only", e["readers"], enc)

    def test_a_missing_encoding_is_refused(self):
        cfg = copy.deepcopy(_config())
        del cfg["expect"]["hdr-pq-tiff"]
        self.refused(cfg, "no `expect` entry for hdr-pq-tiff")

    def test_an_unknown_reader_is_refused(self):
        cfg = copy.deepcopy(_config())
        cfg["expect"]["sdr-tiff"]["readers"].append("android")
        self.refused(cfg, "unknown reader(s) android")

    def test_a_reader_without_settings_is_refused(self):
        cfg = copy.deepcopy(_config())
        cfg["readers"]["chrome"]["settings"] = {}
        self.refused(cfg, "readers.chrome needs a `label` and `settings`")

    def test_an_expectation_missing_a_field_is_refused(self):
        cfg = copy.deepcopy(_config())
        del cfg["expect"]["sdr-tiff"]["fallback"]
        self.refused(cfg, "expect.sdr-tiff needs")

    def test_an_input_needs_one_source(self):
        cfg = copy.deepcopy(_config())
        cfg["inputs"]["bad"] = {"roll": "x"}
        self.refused(cfg, "input 'bad' needs exactly one of")

    def test_a_destination_block_with_a_recipe_is_refused(self):
        with open(compare.BENCHMARK) as f:
            bench = json.load(f)
        case = next(c for c in bench["sets"]["fixtures"]["cases"]
                    if "--film-master" in c["destination"]["args"])
        case["destination"]["recipe"] = "scripts/real-scan-verify/recipes/Ektar.json"
        path = os.path.join(self.tmp, "benchmark.json")
        with open(path, "w") as f:
            json.dump(bench, f)
        with self.assertRaises(viewer.ViewerError) as e:
            viewer.destinations(_config(), path)
        self.assertIn("names a recipe", str(e.exception))

    def test_a_roll_without_a_leader_is_refused_before_measuring(self):
        assets = {"rolls": {"r": {"frames": [
            {"file": "rolls/r/1.tif", "role": "real"},
            {"file": "rolls/r/base.tif", "role": "unexposed"}]}}}
        with self.assertRaises(viewer.ViewerError) as e:
            viewer.roll_recipe("/nonexistent/hanten", assets, self.tmp, "r", self.tmp)
        self.assertIn("one leader", str(e.exception))

    def test_a_frame_missing_from_its_roll_is_refused(self):
        cfg = copy.deepcopy(_config())
        cfg["inputs"]["x"] = {"roll": "r", "frame": "9"}
        assets = {"rolls": {"r": {"frames": [{"file": "rolls/r/1.tif", "role": "real"}]}}}
        with self.assertRaises(viewer.ViewerError) as e:
            viewer.input_argv(cfg, "x", "/nonexistent/hanten", self.tmp, assets, self.tmp)
        self.assertIn("no single frame '9'", str(e.exception))

    def test_the_destinations_are_the_ready_rows_and_the_film_master(self):
        dests = viewer.destinations(_config(), compare.BENCHMARK)
        self.assertEqual({d["id"] for d in dests}, DESTINATIONS)
        self.assertEqual(len(dests), len(DESTINATIONS))


def _tiff(width: int, height: int, order: str = "<", ifd_last: bool = True) -> bytes:
    """A classic TIFF whose IFD sits after the pixel data, as nc writes it."""
    pixels = b"\0" * (width * height * 3)
    ifd_at = 8 + len(pixels) if ifd_last else 8
    entries = [(256, 4, 1, width), (257, 3, 1, height)]
    ifd = struct.pack(order + "H", len(entries))
    for tag, typ, count, value in entries:
        packed = struct.pack(order + ("I" if typ == 4 else "H"), value).ljust(4, b"\0")
        ifd += struct.pack(order + "HHI", tag, typ, count) + packed
    ifd += b"\0\0\0\0"
    head = (b"II" if order == "<" else b"MM") + struct.pack(order + "HI", 42, ifd_at)
    return head + (pixels + ifd if ifd_last else ifd + pixels)


def _jpeg(width: int, height: int) -> bytes:
    app0 = b"\xff\xe0" + struct.pack(">H", 16) + b"JFIF\0" + b"\0" * 9
    sof = b"\xff\xc0" + struct.pack(">HBHHB", 11, 8, height, width, 1) + b"\x01\x11\x00"
    return b"\xff\xd8" + app0 + sof + b"\xff\xda\x00\x02" + b"\xff\xd9"


class ImageSize(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="nc-viewer-test-")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def size(self, data: bytes):
        path = os.path.join(self.tmp, "img")
        with open(path, "wb") as f:
            f.write(data)
        return viewer.image_size(path)

    def test_a_tiff_with_its_ifd_past_a_megabyte(self):
        self.assertEqual(self.size(_tiff(700, 600)), (700, 600))  # 1.26 MB of pixels

    def test_a_big_endian_tiff(self):
        self.assertEqual(self.size(_tiff(5, 3, order=">", ifd_last=False)), (5, 3))

    def test_a_jpeg_reads_its_frame_header(self):
        self.assertEqual(self.size(_jpeg(4921, 3321)), (4921, 3321))

    def test_garbage_and_truncation_are_value_errors(self):
        for data in (b"not an image", _tiff(5, 3)[:-10], _jpeg(4, 4)[:24]):
            with self.assertRaises((ValueError, struct.error)):
                self.size(data)


# Captured from scripts/iso-decoder-oracle on macOS 26.6.2 (2026-10-04), object
# addresses redacted: the per-channel values sit inside one array-valued meta line.
APPLE_OK = """=== f.jpg ===
  images in container: 1
  ISO 21496-1 gain map (kCGImageAuxiliaryDataTypeISOGainMap): PRESENT
      description: {BytesPerRow: 2560, Height: 2, PixelFormat: 875836518, Width: 2}
      meta: HDRToneMap:AlternateHeadroom = 2.300448
      meta: HDRToneMap:BaseColorIsWorkingColor = True
      meta: HDRToneMap:BaseHeadroom = 0.000000
      meta: HDRToneMap:ChannelMetadata = [<CGImageMetadataTag 0x0> HDRToneMap:[0] = {
    AlternateOffset = "<CGImageMetadataTag 0x0> HDRToneMap:AlternateOffset = 0.015625";
    BaseOffset = "<CGImageMetadataTag 0x0> HDRToneMap:BaseOffset = 0.015625";
    GainMapMax = "<CGImageMetadataTag 0x0> HDRToneMap:GainMapMax = 1.000000";
    GainMapMin = "<CGImageMetadataTag 0x0> HDRToneMap:GainMapMin = -0.010689";
    Gamma = "<CGImageMetadataTag 0x0> HDRToneMap:Gamma = 1.000000";
}, <CGImageMetadataTag 0x0> HDRToneMap:[1] = {
    AlternateOffset = "<CGImageMetadataTag 0x0> HDRToneMap:AlternateOffset = 0.015625";
    BaseOffset = "<CGImageMetadataTag 0x0> HDRToneMap:BaseOffset = 0.015625";
    GainMapMax = "<CGImageMetadataTag 0x0> HDRToneMap:GainMapMax = 1.584963";
    GainMapMin = "<CGImageMetadataTag 0x0> HDRToneMap:GainMapMin = -0.202787";
    Gamma = "<CGImageMetadataTag 0x0> HDRToneMap:Gamma = 1.000000";
}, <CGImageMetadataTag 0x0> HDRToneMap:[2] = {
    AlternateOffset = "<CGImageMetadataTag 0x0> HDRToneMap:AlternateOffset = 0.015625";
    BaseOffset = "<CGImageMetadataTag 0x0> HDRToneMap:BaseOffset = 0.015625";
    GainMapMax = "<CGImageMetadataTag 0x0> HDRToneMap:GainMapMax = 2.000000";
    GainMapMin = "<CGImageMetadataTag 0x0> HDRToneMap:GainMapMin = -0.243829";
    Gamma = "<CGImageMetadataTag 0x0> HDRToneMap:Gamma = 1.000000";
}]
      meta: HDRToneMap:Version = 1
  Apple/legacy HDR gain map (kCGImageAuxiliaryDataTypeHDRGainMap): ABSENT
  base.PixelWidth = 4
  base.PixelHeight = 3
  SDR decode: 4x3, headroom 1.0
  HDR decode: 4x3, headroom 4.926107
"""
ULTRAHDR_OK = """Ultra HDR Image: Yes
GainMap Metadata:
--maxContentBoost 2 3 4
--minContentBoost 1 1 1
"""


class Parsing(unittest.TestCase):
    def test_apple(self):
        facts = viewer.parse_apple(APPLE_OK)
        self.assertEqual(facts["iso"], "PRESENT")
        self.assertEqual(facts["hdr_size"], [4, 3])
        gains = [2.0 ** v for v in facts["gain_map_max_log2"]]
        self.assertTrue(viewer._gains_match(gains, [2, 3, 4]))

    def test_apple_absent(self):
        facts = viewer.parse_apple(APPLE_OK.replace(": PRESENT", ": ABSENT", 1))
        self.assertEqual(facts["iso"], "ABSENT")

    def test_ultrahdr(self):
        facts = viewer.parse_ultrahdr_probe(ULTRAHDR_OK)
        self.assertTrue(facts["ultra_hdr"])
        self.assertEqual(facts["max_content_boost"], [2, 3, 4])

    def test_ultrahdr_ignores_trailing_text(self):
        facts = viewer.parse_ultrahdr_probe("--maxContentBoost 2 3 4 (linear)\n")
        self.assertEqual(facts["max_content_boost"], [2, 3, 4])

    def test_gains_must_be_three_and_in_order(self):
        self.assertFalse(viewer._gains_match([2, 3], [2, 3, 4]))
        self.assertFalse(viewer._gains_match([4, 3, 2], [2, 3, 4]))
        self.assertFalse(viewer._gains_match([2, 3, math.nan], [2, 3, 4]))
        self.assertTrue(viewer._gains_match([2.00001, 3, 4], [2, 3, 4]))


def _record(cfg: dict) -> dict:
    files = []
    for enc, name in (("sdr-tiff", "a.tiff"), ("gain-map-jpeg", "a.jpg")):
        files.append(dict(file=name, width=4, height=3, encoding=enc,
                          expect=cfg["expect"][enc]))
    return dict(identity=dict(nc_version="0.1.0", git_commit="abc", git_dirty=False,
                              pipeline_version=9), files=files)


class Rubric(unittest.TestCase):
    def test_one_table_per_reader_and_setting(self):
        cfg = _config()
        text = viewer.rubric(cfg, _record(cfg))
        tables = sum(len(r["settings"]) for r in cfg["readers"].values())
        self.assertEqual(text.count("\n## "), tables)

    def test_only_the_gain_map_row_is_gated_and_only_with_hdr_off(self):
        cfg = _config()
        text = viewer.rubric(cfg, _record(cfg))
        on = text.split("## macOS Preview — HDR display, HDR on")[1].split("\n## ")[0]
        off = text.split("## macOS Preview — HDR off")[1].split("\n## ")[0]
        self.assertNotIn("**(gated)**", on)
        for row in off.splitlines():
            if row.startswith("| `"):
                self.assertEqual("**(gated)**" in row, "a.jpg" in row, row)

    def test_hdr_off_asks_for_the_fallback_not_the_hdr_rendition(self):
        e = _config()["expect"]
        rendition, fallback = viewer.expectation(e["gain-map-jpeg"], hdr_on=False)
        self.assertTrue(rendition.startswith("n/a"))
        self.assertIn("SDR base", fallback)
        self.assertEqual(viewer.expectation(e["gain-map-jpeg"], hdr_on=True)[1],
                         "n/a (HDR on)")
        # An SDR file shows its rendition with HDR off too.
        self.assertEqual(viewer.expectation(e["sdr-tiff"], hdr_on=False)[0], "SDR")

    def test_chrome_gets_no_tiff(self):
        cfg = _config()
        text = viewer.rubric(cfg, _record(cfg))
        chrome = text.split("## Chrome on macOS")[1].split("\n## ")[0]
        self.assertIn("a.jpg", chrome)
        self.assertNotIn("a.tiff", chrome)


FAKE_APPLE = """#!/usr/bin/env python3
import os, sys, time
mode = os.environ.get("FAKE_MODE", "ok")
if mode == "hang":
    time.sleep(30)
text = TRANSCRIPT
if mode == "absent":
    text = text.replace(": PRESENT", ": ABSENT", 1)
if mode == "swapped":
    text = text.replace("GainMapMax = 1.000000", "GainMapMax = X").replace(
        "GainMapMax = 2.000000", "GainMapMax = 1.000000").replace(
        "GainMapMax = X", "GainMapMax = 2.000000")
print(text)
""".replace("TRANSCRIPT", repr(APPLE_OK))
FAKE_ULTRAHDR = r'''#!/usr/bin/env python3
import os, sys, time
mode = os.environ.get("FAKE_MODE", "ok")
if mode == "hang":
    time.sleep(30)
argv = sys.argv[1:]
if not argv:
    print("## ultra hdr demo application. lib version: v9.9.9")
    sys.exit(0)
if "-P" in argv:
    if mode == "absent":
        print("Ultra HDR Image: No")
        sys.exit(255)
    print("Ultra HDR Image: Yes")
    print("--maxContentBoost 2 3 4")
    sys.exit(0)
out = argv[argv.index("-z") + 1]
size = 4 * 3 * 8 - (8 if mode == "short" else 0)
with open(out, "wb") as f:
    f.write(b"\0" * size)
'''


class Check(unittest.TestCase):
    """`viewer check`'s verdicts against fake decoders over a hand-made set."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="nc-viewer-test-")
        self.set = os.path.join(self.tmp, "set")
        os.makedirs(self.set)
        self.jpg = os.path.join(self.set, "a.jpg")
        with open(self.jpg, "wb") as f:
            f.write(_jpeg(4, 3))
        cfg = _config()
        record = dict(schema_version=viewer.SET_SCHEMA, identity={}, files=[
            dict(file="a.jpg", encoding="gain-map-jpeg", width=4, height=3,
                 sha256=viewer._manifest.sha256(self.jpg), gain_map_max=[2.0, 3.0, 4.0],
                 expect=cfg["expect"]["gain-map-jpeg"])])
        with open(os.path.join(self.set, viewer.SET_FILE), "w") as f:
            json.dump(record, f)
        self.oracle = self.fake("oracle", FAKE_APPLE)
        self.app = self.fake("ultrahdr_app", FAKE_ULTRAHDR)

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def fake(self, name: str, body: str) -> str:
        path = os.path.join(self.tmp, name)
        with open(path, "w") as f:
            f.write(body)
        os.chmod(path, 0o755)
        return path

    def run_check(self, mode: str = "ok") -> tuple[int, dict]:
        with mock.patch.dict(os.environ, {"FAKE_MODE": mode}):
            with mock.patch("sys.stdout"):
                rc = cli.main(["viewer", "check", self.set, "--oracle", self.oracle,
                               "--ultrahdr", self.app])
        path = os.path.join(self.set, viewer.CHECK_FILE)
        if not os.path.exists(path):
            return rc, {}
        with open(path) as f:
            return rc, json.load(f)

    def test_a_good_file_passes_both_decoders(self):
        rc, result = self.run_check()
        self.assertEqual(rc, 0)
        self.assertTrue(result["ok"])
        self.assertEqual(result["decoders"]["libultrahdr"], "v9.9.9")
        self.assertEqual([c["decoder"] for c in result["results"][0]["checks"]],
                         ["apple-imageio", "libultrahdr"])

    def test_a_missing_gain_map_fails(self):
        rc, result = self.run_check("absent")
        self.assertEqual(rc, 1)
        reasons = [r for c in result["results"][0]["checks"] for r in c["reasons"]]
        self.assertIn("ISO gain map ABSENT", reasons)
        self.assertTrue(any(r.startswith("probe did not find") for r in reasons))

    def test_channel_gains_out_of_order_fail(self):
        rc, result = self.run_check("swapped")
        self.assertEqual(rc, 1)
        apple = result["results"][0]["checks"][0]
        self.assertFalse(apple["ok"])
        self.assertTrue(result["results"][0]["checks"][1]["ok"])

    def test_a_short_decode_fails(self):
        rc, result = self.run_check("short")
        self.assertEqual(rc, 1)
        self.assertIn("decoded 88 bytes, expected 4x3x8",
                      result["results"][0]["checks"][1]["reasons"])

    def test_a_file_changed_since_the_set_fails(self):
        with open(self.jpg, "ab") as f:
            f.write(b"\0")
        rc, result = self.run_check()
        self.assertEqual(rc, 1)
        self.assertEqual(result["results"][0]["reasons"],
                         ["file changed since the set was built"])

    def test_a_missing_file_fails(self):
        os.remove(self.jpg)
        rc, result = self.run_check()
        self.assertEqual(rc, 1)
        self.assertEqual(result["results"][0]["reasons"], ["missing from the set"])

    def test_a_hung_decoder_fails_the_file(self):
        with mock.patch.object(viewer, "DECODER_TIMEOUT_S", 1):
            rc, result = self.run_check("hang")
        self.assertEqual(rc, 1)
        self.assertIn("timed out", result["results"][0]["checks"][0]["reasons"])

    def test_a_malformed_entry_is_operational(self):
        path = os.path.join(self.set, viewer.SET_FILE)
        with open(path) as f:
            record = json.load(f)
        record["files"][0]["gain_map_max"] = None
        with open(path, "w") as f:
            json.dump(record, f)
        with mock.patch("sys.stderr"):
            self.assertEqual(self.run_check()[0], 2)

    def test_no_oracle_is_operational_and_drops_the_old_verdict(self):
        self.assertEqual(self.run_check()[0], 0)
        with mock.patch("sys.stderr"):
            rc = cli.main(["viewer", "check", self.set, "--oracle",
                           os.path.join(self.tmp, "missing"), "--ultrahdr", self.app])
        self.assertEqual(rc, 2)
        self.assertFalse(os.path.exists(os.path.join(self.set, viewer.CHECK_FILE)))


def _independent_size(path: str) -> tuple[int, int]:
    """The file's size from a library, not from `viewer.image_size`."""
    if path.endswith(".jpg"):
        from PIL import Image
        with Image.open(path) as im:
            return im.size
    import tifffile
    with tifffile.TiffFile(path) as t:
        h, w = t.pages[0].shape[:2]
        return w, h


try:
    import PIL  # noqa: F401
    import tifffile  # noqa: F401
    HAVE_DEPS = True
except ImportError:  # pragma: no cover - CI sets NCTOOL_REQUIRE_DEPS
    HAVE_DEPS = False


@unittest.skipUnless(HAVE_DEPS, "Pillow/tifffile not installed (scripts/analysis/requirements.txt)")
class SetEndToEnd(unittest.TestCase):
    """The chart through the debug binary: every destination, twice, identically."""

    @classmethod
    def setUpClass(cls):
        assert os.access(NC, os.X_OK), f"{NC} is missing: run `cargo build`"
        cls.tmp = tempfile.mkdtemp(prefix="nc-viewer-test-")
        cls.outs = []
        for k in range(2):
            out = os.path.join(cls.tmp, f"set{k}")
            with mock.patch("sys.stdout"):
                rc = cli.main(["viewer", "set", "--out", out, "--nc", NC,
                               "--input", "chart"])
            assert rc == 0, rc
            cls.outs.append(out)
        with open(os.path.join(cls.outs[0], viewer.SET_FILE)) as f:
            cls.record = json.load(f)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, ignore_errors=True)

    def test_every_destination_and_encoding_is_rendered(self):
        files = self.record["files"]
        self.assertEqual({f["destination"] for f in files}, DESTINATIONS)
        self.assertEqual({f["encoding"] for f in files}, set(acceptance.ENCODINGS))

    def test_entries_describe_their_files(self):
        for f in self.record["files"]:
            path = os.path.join(self.outs[0], f["file"])
            self.assertEqual(viewer._manifest.sha256(path), f["sha256"])
            self.assertEqual(_independent_size(path), (f["width"], f["height"]), f["file"])
            self.assertEqual(f["gain_map_max"] is not None,
                             f["encoding"] == "gain-map-jpeg", f["file"])

    def test_a_second_run_is_byte_identical(self):
        for name in (viewer.SET_FILE, viewer.RUBRIC_FILE):
            with open(os.path.join(self.outs[0], name), "rb") as a, \
                    open(os.path.join(self.outs[1], name), "rb") as b:
                self.assertEqual(a.read(), b.read(), name)

    def test_a_rebuild_needs_force_and_keeps_the_rubric_until_then(self):
        out = self.outs[1]
        rubric = os.path.join(out, viewer.RUBRIC_FILE)
        with open(rubric, "a") as f:
            f.write("answers\n")
        with open(os.path.join(out, viewer.CHECK_FILE), "w") as f:
            f.write("{}")
        with mock.patch("sys.stderr"):
            rc = cli.main(["viewer", "set", "--out", out, "--nc", NC, "--input", "chart"])
        self.assertEqual(rc, 2)
        with open(rubric) as f:
            self.assertTrue(f.read().endswith("answers\n"))
        with mock.patch("sys.stdout"):
            rc = cli.main(["viewer", "set", "--out", out, "--nc", NC, "--input", "chart",
                           "--force"])
        self.assertEqual(rc, 0)
        with open(rubric) as f:
            self.assertFalse(f.read().endswith("answers\n"))
        self.assertFalse(os.path.exists(os.path.join(out, viewer.CHECK_FILE)))

    def test_a_forced_rebuild_of_one_input_removes_the_others(self):
        out = self.outs[1]
        stale = os.path.join(out, "ektar-1627")
        os.makedirs(stale, exist_ok=True)
        with open(os.path.join(stale, "old.jpg"), "w") as f:
            f.write("x")
        with mock.patch("sys.stdout"):
            rc = cli.main(["viewer", "set", "--out", out, "--nc", NC, "--input", "chart",
                           "--input", "chart", "--force"])
        self.assertEqual(rc, 0)
        self.assertFalse(os.path.exists(stale))
        with open(os.path.join(out, viewer.SET_FILE)) as f:
            self.assertEqual(len(json.load(f)["files"]), len(DESTINATIONS))

    def test_an_output_inside_the_repository_is_refused(self):
        out = os.path.join(compare.repo_root(), "viewer-set-test")
        with mock.patch("sys.stderr"):
            rc = cli.main(["viewer", "set", "--out", out, "--nc", NC, "--input", "chart"])
        self.assertEqual(rc, 2)
        self.assertFalse(os.path.exists(out))


if __name__ == "__main__":
    unittest.main()
