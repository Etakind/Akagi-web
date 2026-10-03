#!/usr/bin/env python3
"""Verify all five release inventories and hashes before publication."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def verify_uploaded(directory, metadata, tag):
    data = json.loads(metadata.read_text())
    assert data['draft'] is True and data['tag_name'] == tag, 'Unexpected release identity or visibility'
    assets = data['assets']
    local = {path.name: path for path in directory.iterdir()}
    assert len(assets) == len(local), 'Uploaded asset count mismatch'
    assert {asset['name'] for asset in assets} == set(local), 'Uploaded assets incomplete'
    for asset in assets:
        path = local[asset['name']]
        assert asset['state'] == 'uploaded', 'Asset upload incomplete'
        assert asset['size'] == path.stat().st_size, 'Uploaded asset size mismatch'
        digest = hashlib.sha256()
        with path.open('rb') as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                digest.update(block)
        assert asset['digest'] == 'sha256:' + digest.hexdigest(), 'Uploaded asset digest mismatch'

def verify(directory):
    targets = json.loads((ROOT / 'build/targets.json').read_text())
    version = re.search(r'^version = "([0-9A-Za-z.+-]+)"', (ROOT / 'Cargo.toml').read_text(), re.M).group(1)
    expected = set()
    for target in targets:
        stem = f"akagi-{version}-{target['slug']}"
        manifest = directory / (stem + '.assets.json')
        checksums = directory / (stem + '.sha256')
        assert manifest.is_file() and not manifest.is_symlink(), 'Missing or unsafe inventory'
        data = json.loads(manifest.read_text())
        assert data['target'] == target['target'], 'Inventory target mismatch'
        names = {stem + '.' + ext for ext in target['formats']}
        assert {a['name'] for a in data['assets']} == names, 'Incomplete package formats'
        assert len(data['assets']) == len(names), 'Duplicate inventory entry'
        expected.update(names | {manifest.name, checksums.name})
        lines = []
        for asset in data['assets']:
            path = directory / asset['name']
            assert path.is_file() and not path.is_symlink(), 'Missing or unsafe asset'
            assert path.stat().st_size == asset['bytes'], 'Asset size mismatch'
            with path.open('rb') as stream:
                digest = hashlib.sha256()
                for block in iter(lambda: stream.read(1024 * 1024), b''):
                    digest.update(block)
                digest = digest.hexdigest()
            assert digest == asset['sha256'], 'Asset digest mismatch'
            lines.append(f"{digest}  {path.name}\n")
            if path.suffix == '.zip':
                with zipfile.ZipFile(path) as archive:
                    entries = archive.namelist()
                    binary = 'akagi.exe' if target['os'] == 'windows' else 'akagi'
                    for required in [binary, 'LICENSE.txt', 'NOTICE', 'README.md', 'README.zh-CN.md']:
                        assert f'{stem}/{required}' in entries, 'Required portable package file missing'
                    for entry in entries:
                        parts = entry.split('/')
                        assert parts[0] == stem and '..' not in parts, 'Unsafe archive path'
                        assert parts[1] in [binary, 'LICENSE.txt', 'NOTICE', 'README.md', 'README.zh-CN.md', 'docs'], 'Unexpected runtime data in archive'
                    if target['os'] != 'windows':
                        assert archive.getinfo(f'{stem}/{binary}').external_attr >> 16 & 0o111, 'Portable binary not executable'
        assert checksums.is_file() and not checksums.is_symlink(), 'Missing checksums'
        assert checksums.read_text() == ''.join(lines), 'Checksum file mismatch'
    actual = {p.name for p in directory.iterdir()}
    assert actual - expected <= {name + '.minisig' for name in expected}, 'Unexpected release asset'
    assert expected <= actual, 'Release assets incomplete'

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--uploaded-metadata', type=Path)
    parser.add_argument('--tag')
    args = parser.parse_args()
    verify(args.directory)
    if args.uploaded_metadata:
        if not args.tag:
            parser.error('--tag is required with --uploaded-metadata')
        verify_uploaded(args.directory, args.uploaded_metadata, args.tag)
    print('RELEASE_ASSETS_VERIFIED')
