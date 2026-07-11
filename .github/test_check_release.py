#!/usr/bin/env python3
"""Deterministic tests for the release-PR gate."""

from __future__ import annotations

import importlib.util
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = pathlib.Path(__file__).with_name("check-release.py")
SPEC = importlib.util.spec_from_file_location("check_release", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
check_release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_release)


class VersionLadder(unittest.TestCase):
    def test_accepts_exactly_one_semantic_version_step(self) -> None:
        old = (1, 2, 3)
        for new in ((1, 2, 4), (1, 3, 0), (2, 0, 0)):
            with self.subTest(new=new):
                self.assertTrue(check_release.next_step(old, new))

    def test_rejects_skips_resets_and_unchanged_versions(self) -> None:
        old = (1, 2, 3)
        for new in ((1, 2, 3), (1, 2, 5), (1, 3, 1), (2, 0, 1), (0, 9, 9)):
            with self.subTest(new=new):
                self.assertFalse(check_release.next_step(old, new))


class RegistryIndex(unittest.TestCase):
    def test_uses_the_sparse_index_path(self) -> None:
        self.assertEqual(
            check_release.index_url("beecast-player"),
            "https://index.crates.io/be/ec/beecast-player",
        )

    def test_ignores_non_semver_releases_when_finding_the_latest(self) -> None:
        lines = [
            '{"vers":"0.9.4"}',
            '{"vers":"0.10.0-alpha.1"}',
            '{"vers":"0.9.5"}',
        ]
        self.assertEqual(check_release.published_versions(lines), [(0, 9, 4), (0, 9, 5)])


class ReleaseShape(unittest.TestCase):
    """Exercise `check_shape` against real commits in a scratch repository."""

    def setUp(self) -> None:
        repo = tempfile.mkdtemp(prefix="check-release-")
        self.addCleanup(shutil.rmtree, repo)
        self.addCleanup(os.chdir, os.getcwd())
        os.chdir(repo)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "test")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "commit.gpgsign", "false")
        self.base = self.commit("0.9.5", "Initial state.")

    def git(self, *args: str) -> str:
        return subprocess.check_output(("git",) + args, text=True).strip()

    def commit(self, version: str, subject: str, extra: str | None = None) -> str:
        pathlib.Path("Cargo.toml").write_text('[workspace.package]\nversion = "%s"\n' % version)
        pathlib.Path("Cargo.lock").write_text('version = "%s"\n' % version)
        if extra is not None:
            pathlib.Path(extra).write_text("content\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", subject)
        return self.git("rev-parse", "HEAD")

    def test_a_bump_only_pr_is_a_legal_release(self) -> None:
        self.commit("0.9.6", "Bump to 0.9.6.")
        self.assertEqual(check_release.check_shape(self.base, "HEAD"), ((0, 9, 5), (0, 9, 6)))

    def test_the_explicit_head_survives_a_synthetic_merge_checkout(self) -> None:
        self.commit("0.9.5", "Add a feature.", extra="feature.txt")
        head = self.commit("0.9.6", "Bump to 0.9.6.")
        # Reproduce what CI actually has checked out on pull_request events: GitHub's
        # refs/pull/N/merge commit, whose subject and empty diff-tree are meaningless.
        self.git("checkout", "-q", "-b", "pr-merge", self.base)
        self.git("merge", "-q", "--no-ff", head, "-m", "Merge %s into %s" % (head[:7], self.base[:7]))
        self.assertEqual(check_release.check_shape(self.base, head), ((0, 9, 5), (0, 9, 6)))
        with self.assertRaises(SystemExit):
            check_release.check_shape(self.base, "HEAD")

    def test_a_final_commit_with_code_changes_is_rejected(self) -> None:
        self.commit("0.9.6", "Bump to 0.9.6.", extra="feature.txt")
        with self.assertRaises(SystemExit):
            check_release.check_shape(self.base, "HEAD")

    def test_a_pr_without_a_version_bump_is_rejected(self) -> None:
        self.commit("0.9.5", "Add a feature.", extra="feature.txt")
        with self.assertRaises(SystemExit):
            check_release.check_shape(self.base, "HEAD")


if __name__ == "__main__":
    unittest.main()
