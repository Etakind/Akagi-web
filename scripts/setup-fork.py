#!/usr/bin/env python3
"""Install repository-local fork defaults and a branch-independent push guard."""

import argparse
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
from urllib.parse import unquote, urlsplit

ORIGIN = ("github.com", "etakind/akagi")
UPSTREAM = ("github.com", "shinkuan/akagi")
UPSTREAM_URL = "https://github.com/shinkuan/Akagi"
DISABLED_PUSH = "disabled://upstream-push-disabled"
MARKER = "# akagi-fork-guard-v1"


def repository_identity(value):
    """Compare GitHub SSH/HTTPS spellings without printing supplied URLs."""
    if "://" not in value:
        match = re.fullmatch(r"(?:[^@/:]+@)?([^/:]+):(.+)", value)
        if not match:
            return None
        host, path = match.groups()
    else:
        parsed = urlsplit(value)
        host, path = parsed.hostname or "", parsed.path
    host = host.lower().rstrip(".")
    if host == "www.github.com":
        host = "github.com"
    path = unquote(path).strip("/").lower()
    if path.endswith(".git"):
        path = path[:-4]
    return host, path


def guard(remote, location):
    if remote.lower() in ("upstream", "upsteam") or repository_identity(location) == UPSTREAM:
        print("AKAGI_UPSTREAM_PUSH_BLOCKED: push personal changes to origin.", file=sys.stderr)
        return 1
    return 0


def git(*args, optional=False):
    result = subprocess.run(["git", *args], capture_output=True, text=True)
    if result.returncode and not (optional and result.returncode == 1):
        raise RuntimeError("Git operation failed: " + args[0])
    return result.stdout.strip()


def config(key):
    return git("config", "--get", key, optional=True)


def write_atomic(path, text):
    if path.is_symlink():
        raise RuntimeError("Refusing a symlink in the hook installation")
    fd, name = tempfile.mkstemp(prefix=".akagi-", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as output:
            output.write(text)
        os.chmod(name, 0o700)
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def setup(check=False):
    git("rev-parse", "--show-toplevel")
    common = Path(git("rev-parse", "--git-common-dir")).resolve()
    hooks = Path(git("rev-parse", "--git-path", "hooks")).resolve()
    # Never install into a global/shared hooksPath, nor silently replace it.
    if common not in hooks.parents:
        raise RuntimeError("Shared/external core.hooksPath: not modified; use repository-local hooks first")
    remotes = git("remote").splitlines()
    if "origin" not in remotes or repository_identity(git("remote", "get-url", "origin")) != ORIGIN:
        raise RuntimeError("origin must identify the approved personal repository")
    for url in git("remote", "get-url", "--push", "--all", "origin").splitlines():
        if repository_identity(url) != ORIGIN:
            raise RuntimeError("origin has an unexpected push destination")
    if "upsteam" in remotes and "upstream" in remotes:
        raise RuntimeError("Both upstream and upsteam exist; resolve the ambiguity before setup")
    upstream = "upstream" if "upstream" in remotes else "upsteam" if "upsteam" in remotes else None
    if upstream and repository_identity(git("remote", "get-url", upstream)) != UPSTREAM:
        raise RuntimeError("Unexpected upstream repository")

    hook = hooks / "pre-push"
    backup = hooks / "pre-push.akagi-original"
    installed = hooks / "akagi-fork-guard.py"
    for path in (hook, backup, installed):
        if path.is_symlink() or (path.exists() and not path.is_file()):
            raise RuntimeError("Unsafe hook file type; nothing installed")
    managed = hook.exists() and MARKER in hook.read_text(encoding="utf-8", errors="replace").splitlines()
    if installed.exists() and not managed:
        raise RuntimeError("Existing unmanaged guard file requires review; refusing to overwrite it")
    if not managed and backup.exists():
        raise RuntimeError("Existing hook backup requires review; refusing to overwrite it")
    settings = {
        "remote.pushDefault": "origin",
        "push.default": "simple",
        "push.followTags": "false",
        "pull.ff": "only",
        "branch.main.pushRemote": "origin",
        "branch.dev.pushRemote": "origin",
        "branch.main.remote": "origin",
        "branch.main.merge": "refs/heads/main",
        "branch.dev.remote": "origin",
        "branch.dev.merge": "refs/heads/dev",
        "remote.upstream.pushurl": DISABLED_PUSH,
    }
    wrapper = "\n".join([
        "#!/bin/sh", MARKER,
        f"{shlex.quote(Path(sys.executable).as_posix())} {shlex.quote(installed.as_posix())} --guard \"$@\" || exit $?",
        f"if [ -x {shlex.quote(backup.as_posix())} ]; then",
        f"  exec {shlex.quote(backup.as_posix())} \"$@\"",
        "fi", "exit 0", "",
    ])
    if check:
        valid = upstream == "upstream" and managed and installed.exists()
        valid = valid and hook.read_text(encoding="utf-8") == wrapper and os.access(hook, os.X_OK)
        valid = valid and installed.read_bytes() == Path(__file__).read_bytes()
        valid = valid and git("remote", "get-url", "--push", "--all", "upstream") == DISABLED_PUSH
        valid = valid and all(git("config", "--local", "--get-all", key, optional=True) == value for key, value in settings.items())
        if not valid:
            raise RuntimeError("Fork configuration is missing or stale; rerun setup-fork.py")
        print("AKAGI_FORK_CONFIG_OK")
        return

    hooks.mkdir(parents=True, exist_ok=True)
    if upstream == "upsteam":
        git("remote", "rename", "upsteam", "upstream")
    elif upstream is None:
        git("remote", "add", "upstream", UPSTREAM_URL)
    for key, value in settings.items():
        git("config", "--local", "--replace-all", key, value)
    write_atomic(installed, Path(__file__).read_text(encoding="utf-8"))
    if hook.exists() and not managed:
        hook.rename(backup)
    write_atomic(hook, wrapper)
    print("AKAGI_FORK_CONFIGURED")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify without making changes")
    parser.add_argument("--guard", nargs=2, metavar=("REMOTE", "LOCATION"), help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        if args.guard:
            return guard(*args.guard)
        setup(args.check)
        return 0
    except (OSError, RuntimeError, ValueError) as error:
        # No Git stderr, supplied remote URLs or environment contents are printed.
        message = str(error) if isinstance(error, RuntimeError) else "Local operation failed"
        print("AKAGI_FORK_SETUP_FAILED: " + message, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
