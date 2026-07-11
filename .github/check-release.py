#!/usr/bin/env python3
"""Gate the release-PR contract from ENG-PRINCIPLES §14."""

from __future__ import annotations

import json
import re
import subprocess
import sys
import urllib.request
from typing import Iterable


Version = tuple[int, int, int]
CRATES = ("beecast-dto", "beecast-player", "beecast-page", "beecast")


def run(*args: str) -> str:
    return subprocess.check_output(args, text=True).strip()


def version_at(ref: str) -> Version:
    text = run("git", "show", "%s:Cargo.toml" % ref)
    match = re.search(r'^version = "(\d+)\.(\d+)\.(\d+)"$', text, re.MULTILINE)
    if not match:
        raise SystemExit("workspace version is missing or malformed at %s" % ref)
    return tuple(map(int, match.groups()))


def next_step(old: Version, new: Version) -> bool:
    major, minor, patch = old
    return new in {(major, minor, patch + 1), (major, minor + 1, 0), (major + 1, 0, 0)}


def index_url(crate: str) -> str:
    return "https://index.crates.io/%s/%s/%s" % (crate[:2], crate[2:4], crate)


def published_versions(lines: Iterable[str]) -> list[Version]:
    versions = (json.loads(line)["vers"] for line in lines)
    return [tuple(map(int, version.split("."))) for version in versions if re.fullmatch(r"\d+\.\d+\.\d+", version)]


def latest_published(crate: str) -> Version | None:
    try:
        with urllib.request.urlopen(index_url(crate), timeout=30) as response:
            lines = response.read().decode().splitlines()
    except OSError as error:
        raise SystemExit("cannot read the crates.io index for %s: %s" % (crate, error)) from None
    try:
        versions = published_versions(lines)
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit("malformed crates.io index entry for %s: %s" % (crate, error)) from None
    return max(versions) if versions else None


def check_shape(base: str, head: str) -> tuple[Version, Version]:
    """Enforce the release shape on the PR's own commits (no network involved).

    A PR consisting of nothing but the bump commit is legal by construction here
    (`head^` is then `base`): that is the repair path when a release must be re-cut
    without code changes.
    """
    old, new = version_at(base), version_at(head)
    if not next_step(old, new):
        raise SystemExit("version must advance exactly one patch/minor/major step: %s -> %s" % (old, new))

    last_files = set(
        run("git", "diff-tree", "--no-commit-id", "--name-only", "-r", head).splitlines()
    )
    if not last_files or not last_files <= {"Cargo.toml", "Cargo.lock"}:
        raise SystemExit(
            "the final version-bump commit may touch only Cargo.toml and Cargo.lock; got %s"
            % sorted(last_files)
        )
    if version_at("%s^" % head) != old:
        raise SystemExit("the final commit must contain the version bump")
    expected_subject = "Bump to %s." % ".".join(map(str, new))
    subject = run("git", "show", "-s", "--format=%s", head)
    if subject != expected_subject:
        raise SystemExit("the final commit subject must be `%s`; got `%s`" % (expected_subject, subject))
    return old, new


def main(argv: list[str]) -> None:
    if len(argv) not in (2, 3):
        raise SystemExit("usage: check-release.py <base-sha> [head-sha]")
    # On pull_request events the checkout is GitHub's synthetic merge commit, whose
    # subject is `Merge … into …` and whose diff-tree is empty — every shape check
    # would fail against it. CI therefore passes the PR head explicitly; the default
    # of HEAD is for running the gate by hand on a normal branch tip.
    base, head = argv[1], argv[2] if len(argv) == 3 else "HEAD"
    old, new = check_shape(base, head)

    for crate in CRATES:
        published = latest_published(crate)
        if published != old:
            raise SystemExit("base version %s must equal crates.io latest %s for %s" % (old, published, crate))
    print("release step verified: %s -> %s" % (old, new))


if __name__ == "__main__":
    main(sys.argv)
