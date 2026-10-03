"""Offline package regression with fake binaries; never packages user data."""
import json
import hashlib
import importlib.util
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

class PackageTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='akagi-package-test-')
        self.root = Path(self.tmp.name)
        for name in ['scripts/package.py', 'scripts/verify_release.py', 'build/targets.json']:
            dest = self.root / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, dest)
        (self.root / 'Cargo.toml').write_text('[package]\nname = "fixture"\nversion = "0.1.0"\n')
        for name in ['LICENSE.txt', 'NOTICE', 'README.md', 'README.zh-CN.md']:
            (self.root / name).write_text('package fixture\n')
        (self.root / 'docs').mkdir()
        (self.root / 'docs/FORK_MAINTENANCE.md').write_text('fixture\n')
        self.targets = json.loads((self.root / 'build/targets.json').read_text())

    def tearDown(self):
        self.tmp.cleanup()

    def run_script(self, name, *args):
        return subprocess.run([sys.executable, str(self.root / 'scripts' / name), *args], cwd=self.root, capture_output=True, text=True)

    def test_unknown_target_has_no_side_effects(self):
        result = self.run_script('package.py', '--target', 'unsupported')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / 'dist').exists())

    def test_uploaded_release_rejects_partial_corrupt_or_public_assets(self):
        spec = importlib.util.spec_from_file_location('verify_release', ROOT / 'scripts/verify_release.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        directory = self.root / 'uploads'
        directory.mkdir()
        (directory / 'package.zip').write_bytes(b'fixture')
        metadata = self.root / 'uploaded.json'
        valid = {'draft': True, 'tag_name': 'v0.1.0', 'assets': [{
            'name': 'package.zip', 'state': 'uploaded', 'size': 7,
            'digest': 'sha256:' + hashlib.sha256(b'fixture').hexdigest(),
        }]}
        metadata.write_text(json.dumps(valid))
        module.verify_uploaded(directory, metadata, 'v0.1.0')
        for change in ['missing', 'corrupt', 'pending', 'public', 'wrong_tag']:
            data = json.loads(json.dumps(valid))
            if change == 'missing':
                data['assets'] = []
            elif change == 'corrupt':
                data['assets'][0]['digest'] = 'sha256:incorrect'
            elif change == 'pending':
                data['assets'][0]['state'] = 'starter'
            elif change == 'public':
                data['draft'] = False
            else:
                data['tag_name'] = 'v0.0.0'
            metadata.write_text(json.dumps(data))
            with self.subTest(change=change), self.assertRaises(AssertionError):
                module.verify_uploaded(directory, metadata, 'v0.1.0')

    def test_all_formats_and_hashes_without_retired_report(self):
        for target in self.targets:
            release = self.root / 'target' / target['target'] / 'release'
            release.mkdir(parents=True)
            binary = release / ('akagi.exe' if target['os'] == 'windows' else 'akagi')
            binary.write_bytes(b'fake binary for package test\n')
            binary.chmod(0o755)
            for ext in target['formats']:
                if ext != 'zip':
                    folder = release / 'bundle' / ext
                    folder.mkdir(parents=True)
                    (folder / ('fixture.' + ext)).write_bytes(b'fake native package\n')
            result = self.run_script('package.py', '--target', target['target'])
            self.assertEqual(result.returncode, 0, result.stderr)
        result = self.run_script('verify_release.py', 'dist')
        self.assertEqual(result.returncode, 0, result.stderr)
        archive = next((self.root / 'dist').glob('*.zip'))
        with archive.open('ab') as output:
            output.write(b'tampered')
        self.assertNotEqual(self.run_script('verify_release.py', 'dist').returncode, 0)

if __name__ == '__main__':
    unittest.main()
