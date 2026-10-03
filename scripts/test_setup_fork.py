"""Offline integration tests: temporary Git repositories, never contact GitHub."""

import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("setup-fork.py").resolve()
spec = importlib.util.spec_from_file_location("setup_fork", SCRIPT)
setup_fork = importlib.util.module_from_spec(spec)
spec.loader.exec_module(setup_fork)


class ForkSetupTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="akagi-fork-test-")
        self.root = Path(self.tmp.name)
        self.env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        for key in list(self.env):
            if key.startswith("GIT_CONFIG_KEY_") or key.startswith("GIT_CONFIG_VALUE_") or key in (
                "GIT_CONFIG_COUNT", "GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_INDEX_FILE"
            ):
                self.env.pop(key)
        self.git("init", "-b", "main")
        self.git("remote", "add", "origin", "git@github.com:Etakind/Akagi-web.git")
        self.git("remote", "add", "upsteam", "https://github.com/shinkuan/Akagi")
        self.hooks = self.root / ".git" / "hooks"

    def tearDown(self):
        self.tmp.cleanup()

    def run_command(self, args, input=None):
        return subprocess.run(args, cwd=self.root, env=self.env, text=True, input=input, capture_output=True)

    def git(self, *args):
        result = self.run_command(["git", *args])
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def install(self, *args, success=True):
        result = self.run_command([sys.executable, str(SCRIPT), *args])
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result

    def hook(self, remote, url, input=""):
        return self.run_command(["sh", str(self.hooks / "pre-push"), remote, url], input=input)

    def test_idempotent_and_survives_checkout_without_script(self):
        self.install()
        first = (self.hooks / "pre-push").read_bytes()
        self.install()
        self.install("--check")
        self.assertEqual(first, (self.hooks / "pre-push").read_bytes())
        self.assertEqual(self.git("remote"), "origin\nupstream")
        self.assertEqual(self.git("config", "remote.upstream.pushurl"), setup_fork.DISABLED_PUSH)
        self.assertEqual(self.git("config", "remote.pushDefault"), "origin")
        self.assertEqual(self.git("config", "pull.ff"), "only")
        self.assertFalse((self.root / "scripts").exists())
        self.assertEqual(self.hook("upstream", setup_fork.DISABLED_PUSH).returncode, 1)

    def test_blocks_upstream_spellings_but_allows_origin(self):
        self.install()
        for remote, url in [
            ("upstream", "unrelated"), ("upsteam", "unrelated"),
            ("alias", "git@github.com:shinkuan/Akagi.git"),
            ("alias", "https://github.com/shinkuan/Akagi"),
            ("alias", "ssh://git@github.com:22/shinkuan/Akagi.git"),
            ("alias", "HTTPS://GITHUB.COM/shinkuan/Akagi.git/"),
        ]:
            with self.subTest(remote=remote, url=url):
                self.assertNotEqual(self.hook(remote, url).returncode, 0)
        for url in ["git@github.com:Etakind/Akagi-web.git", "https://github.com/Etakind/Akagi-web"]:
            self.assertEqual(self.hook("origin", url).returncode, 0)

    def test_preserves_hook_arguments_stdin_and_failure(self):
        old = self.hooks / "pre-push"
        old.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > hook-args\ncat > hook-input\nexit 23\n')
        old.chmod(0o755)
        content = old.read_bytes()
        self.install()
        self.install()
        self.assertEqual((self.hooks / "pre-push.akagi-original").read_bytes(), content)
        self.assertEqual(self.hook("origin", "https://github.com/Etakind/Akagi-web", "refs fixture\n").returncode, 23)
        self.assertEqual((self.root / "hook-input").read_text(), "refs fixture\n")
        self.assertEqual((self.root / "hook-args").read_text().splitlines()[0], "origin")
        (self.root / "hook-input").unlink()
        self.assertEqual(self.hook("upstream", setup_fork.DISABLED_PUSH).returncode, 1)
        self.assertFalse((self.root / "hook-input").exists())

    def test_shared_hook_path_not_modified(self):
        external = self.root / "shared-hooks"
        external.mkdir()
        original = external / "pre-push"
        original.write_text("fixture")
        self.git("config", "core.hooksPath", str(external))
        before = (self.root / ".git" / "config").read_bytes()
        self.install(success=False)
        self.assertEqual(original.read_text(), "fixture")
        self.assertEqual((self.root / ".git" / "config").read_bytes(), before)

    def test_ambiguous_remotes_and_backup_not_overwritten(self):
        self.git("remote", "add", "upstream", setup_fork.UPSTREAM_URL)
        self.install(success=False)
        self.git("remote", "remove", "upstream")
        (self.hooks / "pre-push.akagi-original").write_text("fixture")
        self.install(success=False)
        self.assertEqual((self.hooks / "pre-push.akagi-original").read_text(), "fixture")

    def test_new_clone_and_check_detects_tampering(self):
        self.git("remote", "remove", "upsteam")
        self.install("--check", success=False)
        self.install()
        self.install("--check")
        self.git("config", "--add", "remote.upstream.pushurl", "https://github.com/shinkuan/Akagi")
        self.install("--check", success=False)
        self.install()
        self.install("--check")

    def test_wrong_origin_is_rejected_before_writes(self):
        self.git("remote", "set-url", "origin", setup_fork.UPSTREAM_URL)
        before = (self.root / ".git" / "config").read_bytes()
        self.install(success=False)
        self.assertEqual((self.root / ".git" / "config").read_bytes(), before)
        self.assertFalse((self.hooks / "pre-push").exists())

    @unittest.skipUnless(os.name == "posix", "POSIX symlink test")
    def test_symlink_hook_not_overwritten(self):
        victim = self.root / "other-hook"
        victim.write_text("fixture")
        (self.hooks / "pre-push").symlink_to(victim)
        self.install(success=False)
        self.assertEqual(victim.read_text(), "fixture")

    def test_git_push_local_bare_remote_runs_guard_and_previous_hook(self):
        self.install()
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-m", "fixture")
        target = self.root / "receiver.git"
        self.git("init", "--bare", str(target))
        # A real Git push validates hook invocation; the destination is local only.
        self.git("push", str(target), "main")
        self.git("remote", "add", "blocked-test", str(target))
        wrapper = self.hooks / "pre-push"
        wrapper.write_text(wrapper.read_text().replace('--guard "$@"', '--guard upstream "$2"'))
        result = self.run_command(["git", "push", "blocked-test", "main"])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("AKAGI_UPSTREAM_PUSH_BLOCKED", result.stderr)


if __name__ == "__main__":
    unittest.main()
