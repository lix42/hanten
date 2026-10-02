"""Tests for `nctool acceptance` and the decoders it is built on.

The standards modules (`cie`, `rec2100`, `gainmap`, `icc`) are tested against
published values and round trips. The oracles are tested on the real debug binary's
output: every oracle must pass a correct file and fail a deliberately corrupted one —
a wrong ICC profile, a moved APP segment, a swapped channel, a shifted code value.
"""

from __future__ import annotations

import io
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from nctool import acceptance, chart, compare, gainmap, icc  # noqa: E402

try:
    import numpy as np
    import tifffile
    from nctool import cie, rec2100
    HAVE_DEPS = True
except ImportError:  # pragma: no cover - the guard below fails it under CI
    HAVE_DEPS = False

needs_deps = unittest.skipUnless(
    HAVE_DEPS, "numpy/Pillow/tifffile not installed (scripts/analysis/requirements.txt)")
NC = os.path.join(compare.repo_root(), "target", "debug", "hanten")
FIXTURE = os.path.join(compare.repo_root(), "tests", "fixtures", "hdri-64bit.tif")
FILM_BASE = ["--film-base", "0.9,0.55,0.42"]


class DependencyGuard(unittest.TestCase):
    def test_dependencies_present_when_required(self):
        if os.environ.get("NCTOOL_REQUIRE_DEPS") == "1":
            self.assertTrue(HAVE_DEPS, "NCTOOL_REQUIRE_DEPS=1 but the acceptance "
                                       "dependencies are missing")


@needs_deps
class Standards(unittest.TestCase):
    def test_ciede2000_matches_sharma_wu_dalal(self):
        # Pairs 1, 2, 3, 7 and 25 of the published test data.
        pairs = [((50, 2.6772, -79.7751), (50, 0, -82.7485), 2.0425),
                 ((50, 3.1571, -77.2803), (50, 0, -82.7485), 2.8615),
                 ((50, 2.8361, -74.0200), (50, 0, -82.7485), 3.4412),
                 ((50, 0, 0), (50, -1, 2), 2.3669),
                 ((60.2574, -34.0099, 36.2677), (60.4626, -34.1751, 39.4387), 1.2644)]
        for a, b, want in pairs:
            self.assertAlmostEqual(float(cie.delta_e_2000(a, b)), want, places=4)

    def test_lab_white_is_100_and_neutral(self):
        lab = cie.xyz_to_lab(cie.D65_WHITE)
        np.testing.assert_allclose(lab, [100, 0, 0], atol=1e-9)

    def test_uv_refuses_black(self):
        with self.assertRaises(ValueError):
            cie.uv_prime([0, 0, 0])

    def test_pq_endpoints_and_round_trip(self):
        self.assertAlmostEqual(float(rec2100.pq_inverse_eotf(10000)), 1.0, places=12)
        nits = np.array([0.01, 1, 100, 203, 1000, 4000])
        np.testing.assert_allclose(rec2100.pq_eotf(rec2100.pq_inverse_eotf(nits)), nits,
                                   rtol=1e-9)

    def test_hlg_oetf_is_continuous_and_round_trips(self):
        self.assertAlmostEqual(float(rec2100.hlg_oetf(1 / 12)), 0.5, places=12)
        self.assertAlmostEqual(float(rec2100.hlg_oetf(1.0)), 1.0, places=6)
        e = np.linspace(0, 1, 101)
        np.testing.assert_allclose(rec2100.hlg_inverse_oetf(rec2100.hlg_oetf(e)), e,
                                   atol=1e-12)

    def test_hlg_signal_decodes_back_to_display_light(self):
        # In-cube colours survive the inverse OOTF and OOTF unchanged.
        linear = np.array([[0.18, 0.18, 0.18], [1.0, 0.5, 0.25], [2.0, 2.2, 1.9]])
        signal = rec2100.hlg_signal(linear, 203, 1000, 1.2)
        np.testing.assert_allclose(rec2100.hlg_display_nits(signal, 1000, 1.2) / 203,
                                   linear, rtol=1e-9)

    def test_radial_pull_keeps_luminance_and_lands_on_the_cube(self):
        rgb = np.array([1.4, 0.2, 0.1])
        y = rgb @ rec2100.BT2020_LUMA
        out = rec2100.radial_to_cube(rgb, y, 1.0)
        self.assertAlmostEqual(float(out @ rec2100.BT2020_LUMA), float(y), places=12)
        self.assertAlmostEqual(float(out.max()), 1.0, places=12)
        np.testing.assert_allclose(rec2100.radial_to_cube(rgb * 0.5, y * 0.5, 1.0),
                                   rgb * 0.5, rtol=1e-15)

    def test_round_half_away_from_zero(self):
        np.testing.assert_array_equal(rec2100.round_half_away([0.5, 1.5, 2.5, -0.5]),
                                      [1, 2, 3, -1])


def _iso_payload(channels=3, **over) -> bytes:
    """A full-form ISO 21496-1 payload, as the standard lays it out."""
    flags = (0x80 if channels == 3 else 0) | 0x40
    out = struct.pack(">HHB", 0, 0, flags)
    out += struct.pack(">IIII", 0, 1, 2300, 1000)
    for c in range(channels):
        lo = over.get("min", [-100, -50, 0])[c]
        out += struct.pack(">iIiIIIiIiI", lo, 1000, 1500, 1000, 1, 1, 1, 64, 1, 64)
    return out


class GainMapParsing(unittest.TestCase):
    def test_metadata_reads_the_full_form(self):
        md = gainmap.parse_metadata(_iso_payload())
        self.assertEqual(md["channels"], 3)
        self.assertTrue(md["use_base_colour_space"])
        self.assertEqual(md["alternate_hdr_headroom"], 2.3)
        self.assertEqual(md["gain_map_min"], [-0.1, -0.05, 0.0])
        self.assertEqual(md["base_offset"], [1 / 64] * 3)

    def test_one_channel_metadata_applies_to_all_three(self):
        md = gainmap.parse_metadata(_iso_payload(channels=1))
        self.assertEqual(md["gain_map_min"], [-0.1] * 3)

    def test_trailing_bytes_and_zero_denominators_are_refused(self):
        with self.assertRaises(gainmap.GainMapError):
            gainmap.parse_metadata(_iso_payload() + b"\0")
        bad = bytearray(_iso_payload())
        bad[9:13] = b"\0\0\0\0"  # base_hdr_headroom's denominator
        with self.assertRaises(gainmap.GainMapError):
            gainmap.parse_metadata(bytes(bad))

    def test_version_only_payload(self):
        self.assertTrue(gainmap.parse_metadata(b"\0\0\0\0")["version_only"])

    @needs_deps
    def test_reconstruct_with_zero_gain_is_the_base(self):
        md = gainmap.parse_metadata(_iso_payload(min=[0, 0, 0]))
        md["gain_map_max"] = [0.0] * 3
        base = np.full((2, 2, 3), 0.25)
        np.testing.assert_allclose(gainmap.reconstruct(base, np.zeros((2, 2, 3)), md), base)

    @needs_deps
    def test_upsample_is_centre_aligned(self):
        m = np.arange(4, dtype=float).reshape(1, 4, 1)
        np.testing.assert_allclose(gainmap.upsample(m, 1, 2)[0, :, 0], [0.5, 2.5])
        np.testing.assert_allclose(gainmap.upsample(m, 1, 4), m)


class Manifest(unittest.TestCase):
    def setUp(self):
        self.man = acceptance.load_manifest(acceptance.MANIFEST)

    def test_every_benchmark_case_is_a_case_and_the_chart_reuses_the_hdri_ones(self):
        cases = acceptance.resolve(self.man, compare.BENCHMARK)
        bench, _ = compare.load_json(compare.BENCHMARK)
        names = {c["name"] for c in cases}
        fixtures = bench["sets"]["fixtures"]["cases"]
        self.assertTrue({c["name"] for c in fixtures} <= names)
        hdri = [c["name"] for c in fixtures if c["input"].endswith("hdri-64bit.tif")]
        self.assertEqual({n for n in names if n.startswith("chart-")},
                         {"chart-" + n[len("hdri-"):] for n in hdri})
        chart_case = next(c for c in cases if c["name"] == "chart-default")
        self.assertIn("--input-meaning", chart_case["args"])
        self.assertNotIn("hdri-64bit.tif", " ".join(chart_case["args"]))

    def test_untouched_patches_are_chart_patches(self):
        names = set(chart.layout()[2])
        self.assertTrue(set(self.man["cross_encoding"]["untouched"]) <= names)

    def test_an_encoding_without_an_oracle_is_refused(self):
        man = json.loads(json.dumps(self.man))
        man["encodings"]["sdr-jpeg"] = dict(determinism="byte-identical")
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
            json.dump(man, f)
        try:
            with self.assertRaisesRegex(acceptance.AcceptanceError, "no oracle"):
                acceptance.load_manifest(f.name)
        finally:
            os.unlink(f.name)

    def test_a_destination_without_an_oracle_fails_loudly(self):
        report = dict(chain=dict(destination=dict(display=dict(
            range="sdr", transfer="native", gamut="srgb", container="jpeg"))))
        with self.assertRaisesRegex(acceptance.AcceptanceError, "no oracle"):
            acceptance.encoding_of(report)

    @needs_deps
    def test_the_committed_chart_matches_its_generator(self):
        committed = tifffile.imread(os.path.join(compare.repo_root(), "tests", "fixtures",
                                                 "chart-48bit.tif"))
        np.testing.assert_array_equal(committed, chart.pixels())


def _case(name, args, ext="tiff"):
    return dict(name=name, input=FIXTURE, output_ext=ext, recipe=None,
                args=FILM_BASE + list(args))


@needs_deps
class OraclesOnRealOutput(unittest.TestCase):
    """Each oracle passes nc's output and fails a corrupted copy of it."""

    CASES = {
        "sdr-p3": ["--gamut", "display-p3"],
        "sdr-srgb": ["--gamut", "srgb"],
        "linear-p3": ["--range", "hdr", "--transfer", "linear", "--gamut", "display-p3"],
        "pq": ["--range", "hdr", "--transfer", "pq", "--gamut", "bt2020"],
        "hlg": ["--range", "hdr", "--transfer", "hlg", "--gamut", "bt2020"],
        "master": ["--film-master"],
        "gain": ["--range", "hdr", "--gamut", "display-p3", "--container", "jpeg"],
    }

    @classmethod
    def setUpClass(cls):
        assert os.access(NC, os.X_OK), f"{NC} is missing: run `cargo build`"
        cls.man = acceptance.load_manifest(acceptance.MANIFEST)
        cls.tmp = tempfile.mkdtemp(prefix="nc-acceptance-test-")
        cls.runs = {}
        for name, args in cls.CASES.items():
            ext = "jpg" if name == "gain" else "tiff"
            cls.runs[name] = acceptance.convert(NC, _case(name, args, ext), cls.tmp, "t")

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, ignore_errors=True)

    def checks(self, name, output=None):
        run = self.runs[name]
        out = acceptance.check_output(output or run["output"], run["pre_encode"],
                                      run["report"], self.man)
        return {c["check"]: c for c in out}

    def read(self, name) -> bytes:
        with open(self.runs[name]["output"], "rb") as f:
            return f.read()

    def copy(self, name, suffix):
        src = self.runs[name]["output"]
        dst = os.path.join(self.tmp, f"{name}-{suffix}{os.path.splitext(src)[1]}")
        shutil.copyfile(src, dst)
        return dst

    def rewrite_tiff(self, name, suffix, pixels=None, icc_from=None):
        """A copy of `name`'s TIFF with other pixels and/or another file's profile."""
        src = self.runs[name]["output"]
        with tifffile.TiffFile(src) as t:
            data = t.pages[0].asarray()
            profile = t.pages[0].tags["InterColorProfile"].value
        if icc_from:
            with tifffile.TiffFile(self.runs[icc_from]["output"]) as t:
                profile = t.pages[0].tags["InterColorProfile"].value
        dst = os.path.join(self.tmp, f"{name}-{suffix}.tiff")
        tifffile.imwrite(dst, data if pixels is None else pixels, photometric="rgb",
                         iccprofile=profile)
        return dst

    def assert_all_pass(self, checks):
        failed = {k: v for k, v in checks.items() if not v["passed"]}
        self.assertEqual(failed, {})

    def test_every_correct_output_passes(self):
        for name in self.CASES:
            with self.subTest(name):
                self.assert_all_pass(self.checks(name))

    def test_a_rewritten_but_unchanged_tiff_still_passes(self):
        # The corruption tests rewrite with tifffile; this pins that the rewrite alone
        # changes nothing an oracle reads.
        self.assert_all_pass(self.checks("sdr-p3", self.rewrite_tiff("sdr-p3", "same")))

    def test_a_wrong_icc_profile_fails_the_metadata(self):
        for name, other in (("sdr-p3", "sdr-srgb"), ("master", "linear-p3"),
                            ("hlg", "pq"), ("linear-p3", "master")):
            with self.subTest(name):
                got = self.checks(name, self.rewrite_tiff(name, "icc", icc_from=other))
                self.assertFalse(got["metadata"]["passed"])

    def test_a_swapped_channel_fails_the_pixels(self):
        for name in ("sdr-p3", "pq", "linear-p3"):
            with self.subTest(name):
                with tifffile.TiffFile(self.runs[name]["output"]) as t:
                    data = t.pages[0].asarray()
                got = self.checks(name, self.rewrite_tiff(name, "swap",
                                                          pixels=data[..., ::-1].copy()))
                self.assertFalse(got["pixels"]["passed"])

    def test_a_shifted_code_value_fails_the_pixels(self):
        # Three codes: whatever ±1 the sample already carried, it lands at 2 or more.
        for name in ("sdr-p3", "pq", "hlg"):
            with self.subTest(name):
                with tifffile.TiffFile(self.runs[name]["output"]) as t:
                    data = t.pages[0].asarray().copy()
                v = int(data[10, 10, 1])
                data[10, 10, 1] = v + 3 if v < 65532 else v - 3
                got = self.checks(name, self.rewrite_tiff(name, "shift", pixels=data))
                self.assertFalse(got["pixels"]["passed"])
                self.assertGreaterEqual(got["pixels"]["metrics"]["max"][1], 2.0)

    def test_another_profiles_lut_fails_the_lut_check(self):
        # The PQ profile with the HLG profile's A2B0 spliced in: `cicp` still says PQ,
        # so only a reader that decodes through the LUT would see it.
        def profile(name):
            with tifffile.TiffFile(self.runs[name]["output"]) as t:
                return t.pages[0].tags["InterColorProfile"].value

        def a2b0_span(raw):
            for i in range(struct.unpack_from(">I", raw, 128)[0]):
                sig, off, ln = struct.unpack_from(">4sII", raw, 132 + 12 * i)
                if sig == b"A2B0":
                    return off, ln
            raise AssertionError("no A2B0")

        pq, hlg = profile("pq"), profile("hlg")
        (po, pl), (ho, hl) = a2b0_span(pq), a2b0_span(hlg)
        self.assertEqual(pl, hl)
        spliced = pq[:po] + hlg[ho:ho + hl] + pq[po + pl:]
        with tifffile.TiffFile(self.runs["pq"]["output"]) as t:
            data = t.pages[0].asarray()
        path = os.path.join(self.tmp, "pq-lut.tiff")
        tifffile.imwrite(path, data, photometric="rgb", iccprofile=spliced)
        got = self.checks("pq", path)
        self.assertTrue(got["metadata"]["passed"])
        self.assertFalse(got["icc_lut"]["passed"])

    def test_a_compressed_or_multi_page_tiff_fails_the_metadata(self):
        with tifffile.TiffFile(self.runs["sdr-p3"]["output"]) as t:
            data = t.pages[0].asarray()
            profile = t.pages[0].tags["InterColorProfile"].value
        zipped = os.path.join(self.tmp, "sdr-zip.tiff")
        tifffile.imwrite(zipped, data, photometric="rgb", iccprofile=profile,
                         compression="zlib")
        two = os.path.join(self.tmp, "sdr-two.tiff")
        with tifffile.TiffWriter(two) as w:
            w.write(data, photometric="rgb", iccprofile=profile)
            w.write(data, photometric="rgb")
        for path, field in ((zipped, "tiff.compression"), (two, "tiff.pages")):
            with self.subTest(field):
                got = self.checks("sdr-p3", path)
                self.assertFalse(got["metadata"]["passed"])
                self.assertIn(field, [d["field"] for d in got["metadata"]["diffs"]])

    def rewrite_pre(self, name, suffix, edit):
        """A copy of `name`'s pre-encode export with `edit(buffer, array)` applied."""
        pages = []
        with tifffile.TiffFile(self.runs[name]["pre_encode"]) as t:
            for page in t.pages:
                desc = json.loads(page.description)
                data = page.asarray().copy()
                edit(desc["buffer"], data)
                pages.append((page.description, data))
        path = os.path.join(self.tmp, f"{name}-{suffix}.pre.tiff")
        with tifffile.TiffWriter(path) as w:
            for desc, data in pages:
                w.write(data, photometric="rgb", description=desc, metadata=None)
        return path

    def pre_checks(self, name, pre, output=None):
        run = self.runs[name]
        out = acceptance.check_output(output or run["output"], pre, run["report"], self.man)
        return {c["check"]: c for c in out}

    def test_content_light_is_each_pixels_largest_channel(self):
        # A pure red one reference white above the frame's own peak: CTA-861.3 counts
        # its red channel, which becomes MaxCLL; its luminance is about a quarter of it.
        peak = self.checks("pq")["content_light"]["expected"]["max_cll_nits"] / 203 + 1

        def red(buffer, data):
            if buffer == "hdr-linear":
                data[3, 3] = (peak, 0.0, 0.0)
        got = self.pre_checks("pq", self.rewrite_pre("pq", "red", red))["content_light"]
        self.assertFalse(got["passed"])
        self.assertAlmostEqual(got["expected"]["max_cll_nits"], peak * 203, places=2)

    def report_checks(self, name, edit):
        """`name`'s content_light check against a copy of its report with `edit`."""
        run = self.runs[name]
        report = json.loads(json.dumps(run["report"]))
        edit(report)
        out = acceptance.check_output(run["output"], run["pre_encode"], report, self.man)
        return {c["check"]: c for c in out}["content_light"]

    def test_a_misreported_content_light_fails(self):
        for name, block, field, value in (
                ("linear-p3", "hdr_linear_tiff", "max_fall_nits", None),
                ("pq", "hdr_coded_tiff", "max_cll_nits", None),
                ("pq", "hdr_coded_tiff", "max_cll_nits", "578"),
                ("hlg", "hdr_coded_tiff", "max_cll_nits", 203)):
            def edit(report):
                stated = report[block].get(field)
                report[block][field] = value if value is not None else stated + 1
            with self.subTest(name=name, value=value):
                got = self.report_checks(name, edit)
                self.assertFalse(got["passed"])
                self.assertIn(field, got["fails"][0])

    def test_hlg_fields_must_be_omitted_and_values_must_be_numbers(self):
        # A `null` is not an omission, and a JSON boolean is not a nit count.
        for name, value in (("hlg", None), ("pq", True)):
            with self.subTest(name):
                got = self.report_checks(name, lambda report: report["hdr_coded_tiff"]
                                         .__setitem__("max_cll_nits", value))
                self.assertFalse(got["passed"])

    def test_a_missing_report_block_fails_even_for_hlg(self):
        for name, block in (("hlg", "hdr_coded_tiff"), ("linear-p3", "hdr_linear_tiff")):
            with self.subTest(name):
                got = self.report_checks(name, lambda report: report.pop(block))
                self.assertFalse(got["passed"])
                self.assertIn(block, got["fails"][0])

    def test_an_ulp_below_zero_is_legitimate_but_a_negative_sample_is_not(self):
        # nc's `gain_ratio::between` documents ~-1e-17 as fit gamut's tie, clamped.
        def at(value):
            def edit(buffer, data):
                if buffer == "sdr-linear":
                    data[0, 0, 2] = value
            return edit
        tiny = self.pre_checks("gain", self.rewrite_pre("gain", "tiny", at(-1e-17)))
        rows = tiny["metadata"]["fields"]
        self.assertEqual(rows["renditions.invalid_samples"], 0)
        bad = self.pre_checks("gain", self.rewrite_pre("gain", "negative", at(-0.01)))
        self.assertIn("renditions.invalid_samples",
                      [d["field"] for d in bad["metadata"]["diffs"]])

    def test_a_sign_flipped_zero_fails_bit_identity(self):
        pre = self.rewrite_pre("linear-p3", "zero",
                               lambda b, d: d.__setitem__((5, 5, 0), np.float32(0.0)))
        with tifffile.TiffFile(self.runs["linear-p3"]["output"]) as t:
            data = t.pages[0].asarray().copy()
        data[5, 5, 0] = np.float32(-0.0)
        out = self.rewrite_tiff("linear-p3", "negzero", pixels=data)
        got = self.pre_checks("linear-p3", pre, out)
        self.assertEqual(got["pixels"]["metrics"]["max"][0], 0.0)
        self.assertFalse(got["pixels"]["passed"])

    def test_a_base_iso_version_other_than_zero_fails(self):
        data = bytearray(self.read("gain"))
        seg = next(x for x in gainmap.segments(bytes(data))
                   if x["payload"].startswith(gainmap.ISO_URN))
        at = seg["offset"] + 4 + len(gainmap.ISO_URN)
        struct.pack_into(">H", data, at, 1)
        path = self.copy("gain", "version")
        with open(path, "wb") as f:
            f.write(bytes(data))
        got = self.checks("gain", path)
        self.assertIn("base.iso_version", [d["field"] for d in got["metadata"]["diffs"]])

    def test_decoding_light_without_a_profile_is_a_decode_fault(self):
        with tifffile.TiffFile(self.runs["sdr-p3"]["output"]) as t:
            data = t.pages[0].asarray()
        path = os.path.join(self.tmp, "sdr-noicc.tiff")
        tifffile.imwrite(path, data, photometric="rgb")
        facts = acceptance.facts_for(self.runs["sdr-p3"]["report"], self.man)
        with self.assertRaises(acceptance.DECODE_FAULTS):
            acceptance.decode_light(path, facts)

    def test_a_truncated_profile_fails_a_check_instead_of_crashing(self):
        with tifffile.TiffFile(self.runs["sdr-p3"]["output"]) as t:
            data = t.pages[0].asarray()
            profile = t.pages[0].tags["InterColorProfile"].value
        path = os.path.join(self.tmp, "sdr-truncated.tiff")
        tifffile.imwrite(path, data, photometric="rgb", iccprofile=profile[:200])
        got = self.checks("sdr-p3", path)
        self.assertEqual(list(got), ["decode"])
        self.assertFalse(got["decode"]["passed"])

    def test_a_shifted_float_fails_its_bound(self):
        # Both are held to bit identity: one ULP fails the linear TIFF, and a visible
        # step fails the film master the same way.
        for name, step in (("linear-p3", None), ("master", 1e-5)):
            with self.subTest(name):
                with tifffile.TiffFile(self.runs[name]["output"]) as t:
                    data = t.pages[0].asarray().copy()
                data[5, 5, 0] = (np.nextafter(data[5, 5, 0], np.float32(np.inf))
                                 if step is None else data[5, 5, 0] + np.float32(step))
                got = self.checks(name, self.rewrite_tiff(name, "shift", pixels=data))
                self.assertFalse(got["pixels"]["passed"])
                self.assertFalse(got["pixels"]["metrics"]["bit_identical"])

    def test_a_moved_app_segment_fails_the_gain_map(self):
        data = self.read("gain")
        segs = gainmap.segments(data)
        mpf = next(s for s in segs if s["payload"].startswith(gainmap.MPF_ID))
        dqt = next(s for s in segs if s["marker"] == 0xDB)
        size = 4 + len(mpf["payload"])
        blob = data[mpf["offset"]:mpf["offset"] + size]
        # Move the MPF segment after the first DQT: its offsets still count from where
        # the segment used to be, so they no longer reach the gain map.
        without = data[:mpf["offset"]] + data[mpf["offset"] + size:]
        at = dqt["offset"] - size + 4 + len(dqt["payload"])
        moved = without[:at] + blob + without[at:]
        path = self.copy("gain", "moved")
        with open(path, "wb") as f:
            f.write(moved)
        got = self.checks("gain", path)
        self.assertFalse(got["metadata"]["passed"])
        self.assertIn("container", json.dumps(got["metadata"]["diffs"]))

    def test_swapped_gain_map_channels_fail_the_grid(self):
        data = self.read("gain")
        parsed = gainmap.read(data)
        gm = parsed["entries"][1]
        seg = next(s for s in parsed["gain_map_segments"]
                   if s["payload"].startswith(gainmap.ISO_URN))
        payload = seg["payload"][len(gainmap.ISO_URN):]
        head, per = payload[:21], payload[21:]
        n = len(per) // 3
        swapped = head + per[2 * n:] + per[n:2 * n] + per[:n]
        start = gm["offset"] + seg["offset"] + 4 + len(gainmap.ISO_URN)
        corrupt = data[:start] + swapped + data[start + len(payload):]
        path = self.copy("gain", "swapped")
        with open(path, "wb") as f:
            f.write(corrupt)
        got = self.checks("gain", path)
        self.assertFalse(got["metadata"]["passed"])
        self.assertFalse(got["gain_map_grid"]["passed"])

    def test_a_shifted_gain_map_headroom_fails_the_metadata(self):
        data = bytearray(self.read("gain"))
        parsed = gainmap.read(bytes(data))
        gm = parsed["entries"][1]
        seg = next(s for s in parsed["gain_map_segments"]
                   if s["payload"].startswith(gainmap.ISO_URN))
        # alternate_hdr_headroom's numerator: after the versions, flags and base pair.
        at = gm["offset"] + seg["offset"] + 4 + len(gainmap.ISO_URN) + 5 + 8
        n = struct.unpack_from(">I", data, at)[0]
        struct.pack_into(">I", data, at, n + 1000)
        path = self.copy("gain", "headroom")
        with open(path, "wb") as f:
            f.write(bytes(data))
        got = self.checks("gain", path)
        self.assertFalse(got["metadata"]["passed"])

    def test_a_shifted_base_code_fails_the_base_bound(self):
        from PIL import Image
        data = self.read("gain")
        parsed = gainmap.read(data)
        # Re-encode the base 40 codes darker behind the original head (ICC, ISO, MPF),
        # then repoint the MP entries at the original map.
        with Image.open(io.BytesIO(data)) as im:
            base = np.asarray(im).astype(int)
        shifted = np.clip(base - 40, 0, 255).astype(np.uint8)
        buf = io.BytesIO()
        Image.fromarray(shifted).save(buf, "JPEG", quality=95, subsampling=0)
        new_scan = buf.getvalue()
        head_end = next(s for s in parsed["base_segments"]
                        if s["payload"].startswith(gainmap.MPF_ID))
        mpf_end = head_end["offset"] + 4 + len(head_end["payload"])
        new_segs = gainmap.segments(new_scan)
        tail_start = next(s["offset"] for s in new_segs if s["marker"] == 0xDB)
        primary = data[:mpf_end] + new_scan[tail_start:]
        # Repoint the MPF entries: the primary's new size and the map right after it.
        primary = bytearray(primary)
        tiff_at = head_end["offset"] + 4 + len(gainmap.MPF_ID)
        ifd = struct.unpack_from(">I", primary, tiff_at + 4)[0]
        count = struct.unpack_from(">H", primary, tiff_at + ifd)[0]
        for i in range(count):
            tag, _, _, value = struct.unpack_from(">HHII", primary, tiff_at + ifd + 2 + 12 * i)
            if tag == 0xB002:
                struct.pack_into(">I", primary, tiff_at + value + 4, len(primary))
                struct.pack_into(">I", primary, tiff_at + value + 16 + 8,
                                 len(primary) - tiff_at)
        path = self.copy("gain", "dark")
        with open(path, "wb") as f:
            f.write(bytes(primary) + parsed["gain_map_jpeg"])
        got = self.checks("gain", path)
        self.assertFalse(got["base_jpeg"]["passed"])
        self.assertTrue(got["gain_map_grid"]["passed"])


@needs_deps
class Goldens(unittest.TestCase):
    """`--write-golden` keeps what the run did not cover and the history it had."""

    def record(self, **cases):
        return dict(build={}, decoders={}, cases=[
            dict(name=n, pre_encode_sha256=v, output_sha256=v, checks=[
                dict(check="metadata", passed=True, fields={"x": v})]) for n, v in cases.items()])

    def test_a_partial_run_keeps_the_other_cases_and_real_history(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "golden.json")
            acceptance.write_golden(path, self.record(a="1", b="1"))
            acceptance.write_golden(path, self.record(a="2"))
            acceptance.write_golden(path, self.record(a="2"))
            with open(path) as f:
                cases = json.load(f)["cases"]
        self.assertEqual(set(cases), {"a", "b"})
        self.assertEqual(cases["a"]["output_sha256"], "2")
        self.assertEqual(cases["a"]["previous"]["output_sha256"], "1")
        self.assertNotIn("previous", cases["b"])


class ExitCodes(unittest.TestCase):
    def test_a_missing_golden_is_operational(self):
        env = dict(os.environ, PYTHONPATH=os.path.dirname(os.path.dirname(
            os.path.abspath(__file__))))
        proc = subprocess.run([sys.executable, "-m", "nctool", "acceptance", "run",
                               "--nc", NC, "--golden", "/nonexistent/golden.json"],
                              env=env, capture_output=True, text=True)
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("--golden", proc.stderr)

    @needs_deps
    def test_an_unwritable_result_path_is_operational(self):
        env = dict(os.environ, PYTHONPATH=os.path.dirname(os.path.dirname(
            os.path.abspath(__file__))))
        proc = subprocess.run([sys.executable, "-m", "nctool", "acceptance", "run",
                               "--nc", NC, "--case", "hdri-default",
                               "--out", "/nonexistent/dir/result.json"],
                              env=env, capture_output=True, text=True)
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)


class Preflight(unittest.TestCase):
    def test_a_build_without_the_export_is_refused_before_converting(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = os.path.join(tmp, "hanten")
            with open(fake, "w") as f:
                f.write("#!/bin/sh\n"
                        "case \"$1\" in --version) echo 'hanten 0.1.0'; "
                        "echo 'pipeline_version 9';; *) echo 'Usage: convert';; esac\n")
            os.chmod(fake, 0o755)
            with self.assertRaisesRegex(acceptance.AcceptanceError, "export-pre-encode"):
                acceptance.preflight(fake)


@needs_deps
class PatchAndCrossEncodingOracles(unittest.TestCase):
    """The patch and cross-encoding oracles on synthetic decodes of the chart."""

    def setUp(self):
        self.man = acceptance.load_manifest(acceptance.MANIFEST)
        self.width, self.height, self.patches = chart.layout()

    def light(self, shift=None):
        """An XYZ image holding each patch's film RGB as XYZ, optionally shifted."""
        img = np.zeros((self.height, self.width, 3))
        for name, r in self.patches.items():
            v = np.array(r["film_rgb"]) * [0.95, 1.0, 1.09]
            if shift and name in shift:
                v = v * shift[name]
            img[r["y"]:r["y"] + r["h"], r["x"]:r["x"] + r["w"]] = v
        return img

    def group(self, other_light, encoding="sdr-tiff"):
        ref = dict(name="ref", input_name="chart", rendering="default",
                   encoding="hdr-linear-tiff", gamut="bt2020",
                   light=dict(hdr=self.light()), patches=self.patches, inset=8)
        other = dict(ref, name="other", encoding=encoding, gamut="srgb",
                     light=dict(sdr=other_light))
        return acceptance.cross_encoding([ref, other], self.man)[0]

    def test_identical_light_passes(self):
        self.assertTrue(self.group(self.light())["passed"])

    def test_a_tinted_untouched_patch_fails(self):
        got = self.group(self.light({"muted-warm": [1.03, 1.0, 1.0]}))
        self.assertFalse(got["passed"])
        self.assertTrue(all("muted-warm" in f for f in got["fails"]))

    def test_a_neutral_chromaticity_shift_fails_on_u_v(self):
        got = self.group(self.light({"neutral+0": [1.0006, 1.0, 1.0]}))
        self.assertFalse(got["passed"])
        self.assertIn("Δu'v'", " ".join(got["fails"]))

    def test_a_black_neutral_patch_fails_and_keeps_its_row(self):
        got = self.group(self.light({"neutral+0": [0.0, 0.0, 0.0]}))
        self.assertFalse(got["passed"])
        row = next(r for r in got["rows"] if r["patch"] == "neutral+0")
        self.assertIsNone(row["delta_uv"])

    def test_a_mapped_patch_is_reported_not_gated(self):
        got = self.group(self.light({"sat-red": [0.8, 1.0, 1.0]}))
        self.assertTrue(got["passed"])
        row = next(r for r in got["rows"] if r["patch"] == "sat-red")
        self.assertTrue(row["mapped"])

    def test_the_gain_map_allowance_applies_to_its_encoding_only(self):
        tint = self.light({"muted-warm": [1.03, 1.0, 1.0]})
        self.assertFalse(self.group(tint, "sdr-tiff")["passed"])
        self.assertTrue(self.group(tint, "gain-map-jpeg")["passed"])

    def test_a_shifted_ramp_patch_fails_the_patch_bounds(self):
        codes = np.full((self.height, self.width, 3), 100.0)
        moved = codes.copy()
        r = self.patches["neutral+0"]
        moved[r["y"]:r["y"] + r["h"], r["x"]:r["x"] + r["w"], 0] += 3
        bounds = dict(patches=dict(neutral_mean=1, saturated_mean=2, neutral_chroma=1))
        got = acceptance.patch_checks(moved, codes, self.patches, bounds, 8,
                                      acceptance._rgb_chroma)
        self.assertFalse(got[0]["passed"])
        self.assertTrue(acceptance.patch_checks(codes, codes, self.patches, bounds, 8,
                                                acceptance._rgb_chroma)[0]["passed"])


@needs_deps
class RunEndToEnd(unittest.TestCase):
    """The whole manifest on the debug binary: every check passes, twice-run cases are
    byte-identical, and a golden written by one run accepts the next."""

    def test_the_fixtures_pass_and_a_golden_round_trips(self):
        assert os.access(NC, os.X_OK), f"{NC} is missing: run `cargo build`"
        env = dict(os.environ, PYTHONPATH=os.path.dirname(os.path.dirname(
            os.path.abspath(__file__))))
        with tempfile.TemporaryDirectory() as tmp:
            out, golden = os.path.join(tmp, "result.json"), os.path.join(tmp, "golden.json")
            proc = subprocess.run([sys.executable, "-m", "nctool", "acceptance", "run",
                                   "--nc", NC, "--out", out, "--write-golden", golden],
                                  env=env, capture_output=True, text=True)
            self.assertEqual(proc.returncode, 0, proc.stderr)
            with open(out) as f:
                record = json.load(f)
            self.assertTrue(record["passed"])
            self.assertEqual(record["schema_version"], acceptance.RESULT_SCHEMA)
            self.assertIn("pillow", record["decoders"])
            all_cases = {c["name"] for c in record["cases"]}
            encodings = {c["encoding"] for c in record["cases"]}
            self.assertEqual(encodings, set(acceptance.ENCODINGS))
            for c in record["cases"]:
                det = next(x for x in c["checks"] if x["check"] == "determinism")
                self.assertTrue(det["output_identical"], c["name"])
            self.assertTrue(record["cross_encoding"])

            # The golden accepts a rerun of two cases, and refuses an edited entry.
            args = [sys.executable, "-m", "nctool", "acceptance", "run", "--nc", NC,
                    "--case", "hdri-default", "--case", "chart-hdr-native-srgb-jpeg",
                    "--golden", golden, "--out", out]
            proc = subprocess.run(args, env=env, capture_output=True, text=True)
            self.assertEqual(proc.returncode, 0, proc.stderr)
            with open(golden) as f:
                g = json.load(f)
            g["cases"]["hdri-default"]["metadata"]["icc.red"] = [0.7, 0.3]
            with open(golden, "w") as f:
                json.dump(g, f)
            proc = subprocess.run(args, env=env, capture_output=True, text=True)
            self.assertEqual(proc.returncode, 1, proc.stderr)
            with open(out) as f:
                record = json.load(f)
            bad = [x for x in record["golden"] if not x["passed"]]
            self.assertEqual([(x["case"], x["diffs"]) for x in bad],
                             [("hdri-default", ["metadata"])])

            # Re-baselining against the same golden records the edited entry as history.
            proc = subprocess.run(args + ["--write-golden", golden], env=env,
                                  capture_output=True, text=True)
            self.assertEqual(proc.returncode, 1, proc.stderr)
            with open(golden) as f:
                g = json.load(f)
            self.assertEqual(g["cases"]["hdri-default"]["previous"]["metadata"]["icc.red"],
                             [0.7, 0.3])
            # A two-case re-baseline keeps every other case's entry.
            self.assertEqual(set(g["cases"]), all_cases)
            proc = subprocess.run(args, env=env, capture_output=True, text=True)
            self.assertEqual(proc.returncode, 0, proc.stderr)


if __name__ == "__main__":
    unittest.main()
