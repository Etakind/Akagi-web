#!/usr/bin/env python3
"""离线验证应用裁剪边界，不读取任何本机配置、会话或账号。"""
import pathlib
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class WebOnlyTests(unittest.TestCase):
    def test_unsupported_packaging_target_has_no_side_effect(self):
        for script in ("fetch-runtime.sh", "package-zip.sh"):
            for target in ("x86_64-unknown-linux-gnu", "x86_64-apple-darwin", "../../escape"):
                with self.subTest(script=script, target=target), tempfile.TemporaryDirectory() as tmp:
                    root = pathlib.Path(tmp)
                    (root / "scripts").mkdir()
                    shutil.copyfile(ROOT / "scripts" / script, root / "scripts" / script)
                    (root / "Cargo.toml").write_text('[package]\nversion = "0.0.0"\n')
                    result = subprocess.run(["bash", str(root / "scripts" / script), target],
                                            cwd=root, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 2)
                    self.assertFalse((root / "dist").exists())
                    self.assertFalse((root / "runtime").exists())

    def test_build_workflows_only_package_supported_targets(self):
        for name in ("release.yml", "pr-build.yml"):
            workflow = (ROOT / ".github/workflows" / name).read_text()
            self.assertNotIn("x86_64-unknown-linux-gnu", workflow)
            self.assertNotIn("Linux system deps", workflow)
            self.assertIn("aarch64-apple-darwin", workflow)
            self.assertIn("x86_64-pc-windows-msvc", workflow)
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("os: [macos-14, windows-latest]", ci)
        self.assertIn("cargo test --locked --all-targets", ci)

    def test_retired_runtime_modules_are_absent(self):
        for path in ("src/proxy/mod.rs", "src/capture/hudsucker_backend.rs",
                     "src/bridge/tenhou/mod.rs", "src/bridge/riichi_city/mod.rs",
                     "src/autoplay/tenhou/mod.rs", "src/autoplay/riichi_city/mod.rs"):
            self.assertFalse((ROOT / path).exists(), path)
        manifest = (ROOT / "Cargo.toml").read_text()
        self.assertNotIn("hudsucker =", manifest)
        self.assertNotIn("x509-parser =", manifest)


if __name__ == "__main__":
    unittest.main()
