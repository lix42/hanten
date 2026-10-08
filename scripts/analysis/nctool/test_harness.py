"""Black-box regression tests for ``real-scan-verify/harness.sh``.

Hermetic: a temporary one-roll asset tree is populated from the committed TIFF
fixtures. The happy path invokes the real debug ``hanten`` binary; the failure path
uses a fake binary that exits successfully while writing the wrong container,
reproducing the silent 2026-08-09 regression without reading Drive assets.
"""
from __future__ import annotations

import json
import os
import pathlib
import shutil
import stat
import subprocess
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[3]
HARNESS = ROOT / "scripts" / "real-scan-verify" / "harness.sh"
FIXTURES = ROOT / "tests" / "fixtures"


def scalar_shape(value):
    """Return JSON's structural shape while ignoring values and key order."""
    if isinstance(value, dict):
        return {key: scalar_shape(child) for key, child in value.items()}
    if isinstance(value, list):
        return [scalar_shape(child) for child in value]
    return type(value).__name__


class HarnessTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = pathlib.Path(self._tmp.name)
        self.assets = self.tmp / "assets"
        self.roll = self.assets / "rolls" / "FixtureRoll"
        self.roll.mkdir(parents=True)
        frames = [
            ("unexposed.tif", "hdr-48bit.tif", "unexposed", False),
            ("leader.tif", "hdr-48bit.tif", "leader", False),
            ("real.tif", "hdri-64bit.tif", "real", True),
        ]
        manifest_frames = []
        for name, fixture, role, ir_present in frames:
            shutil.copyfile(FIXTURES / fixture, self.roll / name)
            manifest_frames.append(
                {
                    "file": f"rolls/FixtureRoll/{name}",
                    "role": role,
                    "ir_present": ir_present,
                }
            )
        (self.assets / "manifest.json").write_text(
            json.dumps(
                {"schema_version": 1, "rolls": {"FixtureRoll": {"frames": manifest_frames}}}
            ),
            encoding="utf-8",
        )
        self.recipes = self.tmp / "recipes"
        self.out = self.tmp / "out"
        self.artifacts = self.tmp / "artifacts"

    def run_harness(self, stage, nc, extra_env=None):
        env = os.environ.copy()
        env.update(
            NC=str(nc),
            A=str(self.assets),
            OUTDIR=str(self.out),
            ART=str(self.artifacts),
            REC=str(self.recipes),
        )
        if extra_env:
            env.update(extra_env)
        return subprocess.run(
            ["bash", str(HARNESS), stage],
            cwd=ROOT,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

    def test_fixture_freeze_and_convert_match_the_recipe_and_suffix_contract(self):
        nc = ROOT / "target" / "debug" / "hanten"
        self.assertTrue(nc.is_file(), "run `cargo build` before the harness tests")
        self.assertIsNotNone(shutil.which("jq"), "the harness requires jq")

        freeze = self.run_harness("freeze", nc)
        self.assertEqual(freeze.returncode, 0, freeze.stdout + freeze.stderr)

        generated = json.loads((self.recipes / "FixtureRoll.json").read_text())
        generated_hdr = json.loads((self.recipes / "FixtureRoll.hdr.json").read_text())
        committed = json.loads(
            (ROOT / "scripts/real-scan-verify/recipes/Ektar.json").read_text()
        )
        committed_hdr = json.loads(
            (ROOT / "scripts/real-scan-verify/recipes/Ektar.hdr.json").read_text()
        )
        self.assertEqual(scalar_shape(generated), scalar_shape(committed))
        self.assertEqual(scalar_shape(generated_hdr), scalar_shape(committed_hdr))
        self.assertEqual(generated["recipe_version"], 2)
        self.assertEqual(
            generated["output"],
            {"display": {"range": "sdr", "transfer": "native", "gamut": "display-p3",
                         "container": "tiff"}},
        )
        self.assertEqual(
            generated_hdr["output"],
            {"display": {"range": "hdr", "transfer": "linear", "gamut": "bt2020",
                         "container": "tiff"}},
        )

        convert = self.run_harness("convert", nc)
        self.assertEqual(convert.returncode, 0, convert.stdout + convert.stderr)
        self.assertIn("converted FixtureRoll: 1 frames x2 modes", convert.stdout)
        roll_out = self.out / "FixtureRoll"
        expected = {"real_positive.tiff", "real_positive_hdr.tiff"}
        self.assertEqual({p.name for p in roll_out.iterdir()}, expected)
        for report_name, expected_output in (
            ("FixtureRoll.roll16.json", roll_out / "real_positive.tiff"),
            ("FixtureRoll.rollhdr.json", roll_out / "real_positive_hdr.tiff"),
        ):
            report_text = (self.artifacts / report_name).read_text()
            self.assertNotIn(".rsv-", report_text)
            report = json.loads(report_text)
            self.assertEqual(report["frames"][0]["output"], str(expected_output))
            self.assertTrue(pathlib.Path(report["frames"][0]["output"]).is_file())

    def test_successful_wrong_container_is_rejected_before_publication(self):
        self.recipes.mkdir()
        for suffix in (".json", ".hdr.json"):
            (self.recipes / f"FixtureRoll{suffix}").write_text("{}", encoding="utf-8")

        fake_nc = self.tmp / "fake-nc"
        fake_nc.write_text(
            """#!/usr/bin/env bash
set -eu
out_dir=
params=
previous=
for arg in "$@"; do
  if [ "$previous" = "--out-dir" ]; then out_dir=$arg; previous=; continue; fi
  if [ "$previous" = "--params" ]; then params=$arg; previous=; continue; fi
  previous=$arg
done
mkdir -p "$out_dir"
case "$params" in
  *.hdr.json)
    cp "$FAKE_TIFF_SOURCE" "$out_dir/real_positive.jpg"
    ;;
  *)
    cp "$FAKE_TIFF_SOURCE" "$out_dir/real_positive.tiff"
    ;;
esac
printf '{"command":"roll","frames":[]}\n'
""",
            encoding="utf-8",
        )
        fake_nc.chmod(fake_nc.stat().st_mode | stat.S_IXUSR)

        result = self.run_harness(
            "convert", fake_nc, {"FAKE_TIFF_SOURCE": str(FIXTURES / "hdr-48bit.tif")}
        )
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("float TIFF for FixtureRoll/real.tif was not produced", result.stderr)
        self.assertNotIn("converted FixtureRoll", result.stdout)
        roll_out = self.out / "FixtureRoll"
        self.assertFalse((roll_out / "real_positive.jpg").exists())
        self.assertFalse((roll_out / "real_positive.tiff").exists())

    def write_successful_fake_roll_nc(self):
        fake_nc = self.tmp / "fake-roll-nc"
        fake_nc.write_text(
            """#!/usr/bin/env bash
set -eu
out_dir=
previous=
for arg in "$@"; do
  if [ "$previous" = "--out-dir" ]; then out_dir=$arg; previous=; continue; fi
  previous=$arg
done
mkdir -p "$out_dir"
cp "$FAKE_TIFF_SOURCE" "$out_dir/real_positive.tiff"
if [ "${FAKE_EMPTY_REPORT:-false}" = true ]; then
  printf '{"command":"roll","frames":[]}\n'
else
  printf '{"command":"roll","frames":[{"input":"real.tif","output":"%s/real_positive.tiff","status":"ok"}]}\n' "$out_dir"
fi
""",
            encoding="utf-8",
        )
        fake_nc.chmod(fake_nc.stat().st_mode | stat.S_IXUSR)
        return fake_nc

    def seed_recipes(self):
        self.recipes.mkdir(exist_ok=True)
        for suffix in (".json", ".hdr.json"):
            (self.recipes / f"FixtureRoll{suffix}").write_text("{}", encoding="utf-8")

    def test_same_suffix_non_tiff_is_rejected_before_publication(self):
        self.seed_recipes()
        fake_nc = self.tmp / "fake-bad-content-nc"
        fake_nc.write_text(
            """#!/usr/bin/env bash
set -eu
out_dir=
previous=
for arg in "$@"; do
  if [ "$previous" = "--out-dir" ]; then out_dir=$arg; previous=; continue; fi
  previous=$arg
done
mkdir -p "$out_dir"
printf 'this is not a tiff\n' > "$out_dir/real_positive.tiff"
printf '{"command":"roll","frames":[{"input":"real.tif","output":"%s/real_positive.tiff","status":"ok"}]}\n' "$out_dir"
""",
            encoding="utf-8",
        )
        fake_nc.chmod(fake_nc.stat().st_mode | stat.S_IXUSR)

        result = self.run_harness("convert", fake_nc)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("is not a TIFF container", result.stderr)
        self.assertNotIn("converted FixtureRoll", result.stdout)
        self.assertFalse((self.out / "FixtureRoll" / "real_positive.tiff").exists())

    def test_directory_and_directory_symlink_targets_are_rejected(self):
        self.seed_recipes()
        fake_nc = self.write_successful_fake_roll_nc()
        roll_out = self.out / "FixtureRoll"
        roll_out.mkdir(parents=True)
        env = {"FAKE_TIFF_SOURCE": str(FIXTURES / "hdr-48bit.tif")}

        target = roll_out / "real_positive.tiff"
        target.mkdir()
        result = self.run_harness("convert", fake_nc, env)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("publication target is a directory", result.stderr)
        self.assertEqual(list(target.iterdir()), [])
        self.assertFalse((roll_out / "real_positive_hdr.tiff").exists())

        target.rmdir()
        backing = self.tmp / "target-directory"
        backing.mkdir()
        target.symlink_to(backing, target_is_directory=True)
        result = self.run_harness("convert", fake_nc, env)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("publication target is a directory", result.stderr)
        self.assertEqual(list(backing.iterdir()), [])
        self.assertFalse((roll_out / "real_positive_hdr.tiff").exists())

    def test_missing_successful_report_frame_is_rejected_before_publication(self):
        self.seed_recipes()
        fake_nc = self.write_successful_fake_roll_nc()
        env = {
            "FAKE_TIFF_SOURCE": str(FIXTURES / "hdr-48bit.tif"),
            "FAKE_EMPTY_REPORT": "true",
        }

        result = self.run_harness("convert", fake_nc, env)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("staged 16-bit TIFF", result.stderr)
        self.assertIn("not represented exactly once", result.stderr)
        self.assertNotIn("converted FixtureRoll", result.stdout)
        self.assertFalse((self.out / "FixtureRoll" / "real_positive.tiff").exists())

    def test_ir_stage_rejects_a_warning_about_the_carried_plane(self):
        self.seed_recipes()
        fake_nc = self.tmp / "fake-ir-nc"
        fake_nc.write_text(
            """#!/usr/bin/env bash
set -eu
previous=
for arg in "$@"; do
  if [ "$previous" = "-o" ]; then
    cp "$FAKE_TIFF_SOURCE" "$arg"
  fi
  previous=$arg
done
case "${FAKE_IR:-silent}" in
  warns) echo 'hanten: warning: input carries an IR plane' >&2 ;;
  layout) echo 'hanten: warning: file has additional IFDs beyond the IR plane; ignored' >&2 ;;
  fails) echo 'error: simulated failure' >&2; exit 4 ;;
esac
printf '{"command":"convert"}\n'
""",
            encoding="utf-8",
        )
        fake_nc.chmod(fake_nc.stat().st_mode | stat.S_IXUSR)
        env = {"FAKE_TIFF_SOURCE": str(FIXTURES / "hdr-48bit.tif")}

        result = self.run_harness("ir", fake_nc, {**env, "FAKE_IR": "fails"})
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("converting the IR frame failed", result.stderr)

        result = self.run_harness("ir", fake_nc, {**env, "FAKE_IR": "warns"})
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("the carried IR plane raised a warning", result.stderr)

        # A decoder layout note naming the plane is not the retired warning.
        for fake_ir in ("silent", "layout"):
            result = self.run_harness("ir", fake_nc, {**env, "FAKE_IR": fake_ir})
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("IR plane carried without a warning", result.stdout)


if __name__ == "__main__":
    unittest.main()
