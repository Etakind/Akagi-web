#!/usr/bin/env python3
"""Start the packaged CLI on its native runner without loading user configuration."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', required=True)
    args = parser.parse_args()
    targets = json.loads((ROOT / 'build/targets.json').read_text(encoding='utf-8'))
    target = next((item for item in targets if item['target'] == args.target), None)
    if target is None:
        parser.error('unsupported target')
    version = re.search(r'^version = "([0-9A-Za-z.+-]+)"', (ROOT / 'Cargo.toml').read_text(encoding='utf-8'), re.M).group(1)
    stem = f"akagi-{version}-{target['slug']}"
    binary = 'akagi.exe' if target['os'] == 'windows' else 'akagi'
    with tempfile.TemporaryDirectory(prefix='akagi-release-smoke-') as temporary:
        executable = Path(temporary) / binary
        with zipfile.ZipFile(ROOT / 'dist' / (stem + '.zip')) as archive:
            assert archive.testzip() is None, 'Corrupt portable archive'
            executable.write_bytes(archive.read(f'{stem}/{binary}'))
        executable.chmod(0o755)
        # clap exits before config/log/history loading and before any browser connection.
        result = subprocess.run([str(executable), '--help'], cwd=temporary,
                                capture_output=True, text=True, timeout=30, check=True)
        assert 'Usage: akagi' in result.stdout and '--config' in result.stdout, 'Packaged CLI did not start'
        if target['os'] == 'linux':
            dependencies = subprocess.run(['ldd', str(executable)], capture_output=True, text=True, check=True)
            assert 'not found' not in dependencies.stdout, 'Unresolved shared library'
            deb = subprocess.check_output(['dpkg-deb', '-f', str(ROOT / 'dist' / (stem + '.deb')), 'Architecture'], text=True).strip()
            rpm = subprocess.check_output(['rpm', '-qp', '--queryformat', '%{ARCH}', str(ROOT / 'dist' / (stem + '.rpm'))], text=True).strip()
            assert deb == ('amd64' if target['arch'] == 'x86_64' else 'arm64'), 'Wrong DEB architecture'
            assert rpm == target['arch'], 'Wrong RPM architecture'
    print('PACKAGED_EXECUTABLE_VERIFIED', args.target)

if __name__ == '__main__':
    main()
