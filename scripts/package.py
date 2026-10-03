#!/usr/bin/env python3
"""Package only explicitly supported targets. No runtime downloads or user data."""
import argparse
import hashlib
import json
import pathlib
import re
import shutil
import stat
import tempfile
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
TARGETS = json.loads((ROOT / 'build/targets.json').read_text(encoding='utf-8'))

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--matrix', action='store_true')
    parser.add_argument('--target')
    args = parser.parse_args()
    if args.matrix:
        print(json.dumps({'include': TARGETS}, separators=(',', ':')))
        return
    target = next((t for t in TARGETS if t['target'] == args.target), None)
    if target is None:
        parser.error('unsupported target')
    # Validate before creating, deleting, copying or downloading anything.
    version = re.search(r'^version = "([0-9A-Za-z.+-]+)"', (ROOT / 'Cargo.toml').read_text(encoding='utf-8'), re.M).group(1)
    name = f"akagi-{version}-{target['slug']}"
    binary_name = 'akagi.exe' if target['os'] == 'windows' else 'akagi'
    release = ROOT / 'target' / target['target'] / 'release'
    binary = release / binary_name
    if not binary.is_file():
        parser.error('build the selected target release binary first')
    dist = ROOT / 'dist'
    if dist.is_symlink():
        parser.error('dist must not be a symlink')
    dist.mkdir(exist_ok=True)
    assets = []
    with tempfile.TemporaryDirectory(prefix='akagi-package-') as temporary:
        stage = pathlib.Path(temporary) / name
        stage.mkdir()
        shutil.copy2(binary, stage / binary_name)
        for file in ('LICENSE.txt', 'NOTICE', 'README.md', 'README.zh-CN.md'):
            shutil.copy2(ROOT / file, stage / file)
        shutil.copytree(ROOT / 'docs', stage / 'docs')
        archive = dist / (name + '.zip')
        if archive.is_symlink():
            parser.error('archive must not be a symlink')
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as out:
            for file in sorted(stage.rglob('*')):
                if file.is_file():
                    info = zipfile.ZipInfo.from_file(file, file.relative_to(stage.parent))
                    info.compress_type = zipfile.ZIP_DEFLATED
                    if file == stage / binary_name and target['os'] != 'windows':
                        info.create_system = 3
                        info.external_attr = (stat.S_IFREG | 0o755) << 16
                    with file.open('rb') as source, out.open(info, 'w') as destination:
                        shutil.copyfileobj(source, destination)
        assets.append(archive)
    for extension in target['formats']:
        if extension == 'zip':
            continue
        sources = list((release / 'bundle' / extension).glob('*.' + extension))
        if len(sources) != 1:
            parser.error(f'expected one {extension} bundle for the selected target')
        dest = dist / f'{name}.{extension}'
        if dest.is_symlink():
            parser.error('bundle destination must not be a symlink')
        shutil.copy2(sources[0], dest)
        assets.append(dest)
    inventory = []
    for asset in assets:
        with asset.open('rb') as stream:
            digest = hashlib.sha256()
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                digest.update(block)
            digest = digest.hexdigest()
        inventory.append({'name': asset.name, 'bytes': asset.stat().st_size, 'sha256': digest})
    for suffix in ('.assets.json', '.sha256'):
        if (dist / (name + suffix)).is_symlink():
            parser.error('metadata destination must not be a symlink')
    (dist / f'{name}.assets.json').write_text(json.dumps({'target': target['target'], 'assets': inventory}, indent=2) + '\n', encoding='utf-8')
    (dist / f'{name}.sha256').write_text(''.join(f"{a['sha256']}  {a['name']}\n" for a in inventory), encoding='utf-8')
    print('PACKAGED', target['target'])

if __name__ == '__main__':
    main()
