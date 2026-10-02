#!/usr/bin/env python3
"""Opt-in local Mahjong Soul session reuse. Never reads account or outputs data.
Copies only the official game's origin-partitioned IndexedDB, not the browser
profile, cookie database, passwords, history, or other sites' storage.
"""
import os
from pathlib import Path
import shutil
import stat
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE = Path.home() / 'Library/Application Support/Microsoft Edge/Default/IndexedDB'
DEST = ROOT / 'edge-game-profile'
ORIGIN = 'https_game.maj-soul.com_0.indexeddb.'


def inventory():
    result = {}
    for directory in SOURCE.glob(ORIGIN + '*'):
        for path in [directory, *directory.rglob('*')]:
            meta = path.lstat()
            if stat.S_ISLNK(meta.st_mode) or meta.st_uid != os.geteuid():
                raise ValueError()
            if path.name in ('LOCK', 'LOG', 'LOG.old'):
                continue
            if stat.S_ISREG(meta.st_mode):
                result[path.relative_to(SOURCE)] = (meta.st_size, meta.st_mtime_ns)
            elif not stat.S_ISDIR(meta.st_mode):
                raise ValueError()
    if not result:
        raise ValueError()
    return result


def main():
    os.umask(0o077)
    if DEST.exists() or DEST.is_symlink():
        print('DESTINATION_ALREADY_EXISTS')
        return 1
    snapshot = inventory()
    with tempfile.TemporaryDirectory(prefix='.edge-game-profile-', dir=ROOT) as tmp:
        staged = Path(tmp) / 'profile'
        target = staged / 'Default/IndexedDB'
        target.mkdir(parents=True, mode=0o700)
        for relative in snapshot:
            source = SOURCE / relative
            destination = target / relative
            destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            # No following links even if a source entry changes after inventory.
            source_fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
            with os.fdopen(source_fd, 'rb') as incoming, destination.open('xb') as outgoing:
                shutil.copyfileobj(incoming, outgoing)
        if snapshot != inventory():
            print('SITE_STORAGE_CHANGED_RETRY_LATER')
            return 1
        staged.rename(DEST)
    print('OFFICIAL_SITE_SESSION_PREPARED')
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except Exception:
        print('SITE_SESSION_PREPARATION_FAILED')
        raise SystemExit(1)
