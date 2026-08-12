#!/usr/bin/env python3
"""Bump the project version, verify it builds and packages, then commit and tag.

Cargo.toml's `[package] version` is the single source of truth. The RPM spec
takes its version as a define from the Makefile, so nothing else needs editing
and the two cannot drift.

The ordering matters: the version is committed and tagged first, and the build
runs against that tagged tree — that is what makes the Makefile derive a clean
`-1` RPM release instead of a `.dirty`-suffixed snapshot. A release built
before its commit would permanently carry the previous commit's id plus a
dirty marker, which misdescribes the artifact.

The trade is that a build failure must unwind: the script deletes the tag and
resets the release commit away, restoring the exact pre-bump state. That is
safe because both are local until pushed, and the push instruction is only
printed on success.

Usage:
    scripts/bump-version.py <major|minor|patch> [--dry-run] [--no-verify]
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

# This script lives in scripts/, so the project root is its parent's parent.
PROJECT_ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = PROJECT_ROOT / "Cargo.toml"

# Matches the [package] version line specifically. Anchored to the [package]
# table so a dependency's `version = "..."` is never hit.
PACKAGE_VERSION = re.compile(
    r'(?P<head>\[package\][^\[]*?\bversion\s*=\s*")(?P<version>[^"]+)(?P<tail>")',
    re.DOTALL,
)


class BumpError(RuntimeError):
    """A condition that should stop the bump with a readable message."""


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess:
    """Runs a command, echoing it first so the log shows what happened."""
    print(f"    $ {' '.join(cmd)}", flush=True)
    return subprocess.run(cmd, cwd=PROJECT_ROOT, check=True, **kwargs)


def capture(cmd: list[str]) -> str:
    result = subprocess.run(
        cmd, cwd=PROJECT_ROOT, check=True, capture_output=True, text=True
    )
    return result.stdout.strip()


def read_version() -> str:
    match = PACKAGE_VERSION.search(CARGO_TOML.read_text())
    if not match:
        raise BumpError(
            f"no [package] version found in {CARGO_TOML.name}; "
            "the version is expected to live there"
        )
    return match.group("version")


def bump(version: str, part: str) -> str:
    parts = version.split(".")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        raise BumpError(
            f"current version {version!r} is not a plain MAJOR.MINOR.PATCH; "
            "refusing to guess how to increment it"
        )
    major, minor, patch = (int(p) for p in parts)
    if part == "major":
        return f"{major + 1}.0.0"
    if part == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def write_version(new_version: str) -> None:
    text = CARGO_TOML.read_text()
    updated = PACKAGE_VERSION.sub(
        lambda m: f"{m.group('head')}{new_version}{m.group('tail')}", text, count=1
    )
    if updated == text:
        raise BumpError("version substitution made no change; refusing to continue")
    CARGO_TOML.write_text(updated)


def ensure_clean_worktree() -> None:
    """A dirty tree would sweep unrelated edits into the version commit."""
    status = capture(["git", "status", "--porcelain"])
    # Cargo.lock is regenerated below; ignore it here and stage it explicitly
    # afterwards.
    dirty = [
        line
        for line in status.splitlines()
        if line.strip() and not line.endswith("Cargo.lock")
    ]
    if dirty:
        listing = "\n".join(f"      {line}" for line in dirty)
        raise BumpError(
            "working tree has uncommitted changes; commit or stash them first "
            f"so the version bump stands alone:\n{listing}"
        )


def ensure_tag_available(tag: str) -> None:
    existing = capture(["git", "tag", "--list", tag])
    if existing:
        raise BumpError(f"tag {tag} already exists; refusing to overwrite it")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("part", choices=("major", "minor", "patch"))
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="show what would happen without changing anything",
    )
    parser.add_argument(
        "--no-verify",
        action="store_true",
        help="skip the build and test run (commits and tags an unverified version)",
    )
    args = parser.parse_args()

    try:
        current = read_version()
        new_version = bump(current, args.part)
        tag = f"v{new_version}"

        print(f"==> {current} -> {new_version}")

        if args.dry_run:
            print("    (dry run: nothing written, built, committed or tagged)")
            return 0

        ensure_clean_worktree()
        ensure_tag_available(tag)

        write_version(new_version)
        print(f"==> Wrote {new_version} to {CARGO_TOML.name}")

        # Cargo.lock records the package version; syncing it here keeps the
        # release commit complete, since the build now runs after the commit.
        run(["cargo", "update", "--workspace"])

        # Commit and tag before building: the Makefile derives the RPM release
        # from `git describe`, so building at the tagged commit is what
        # produces a clean `<version>-1` instead of a dirty snapshot id.
        run(["git", "add", "Cargo.toml", "Cargo.lock"])
        run(["git", "commit", "-m", f"Release {new_version}"])
        run(["git", "tag", "-a", tag, "-m", f"Release {new_version}"])

        if args.no_verify:
            print("==> Skipping verification (--no-verify)")
        else:
            # `make package` builds and tests the native architecture, then
            # cross-builds and packages both. On failure, unwind the release:
            # delete the tag and reset the commit so the tree returns to the
            # exact pre-bump state. Both are local-only at this point.
            print("==> Building and packaging at the tagged tree")
            try:
                run(["make", "package"])
            except subprocess.CalledProcessError:
                print("==> Build failed; reverting the release commit and tag")
                run(["git", "tag", "-d", tag])
                run(["git", "reset", "--hard", "HEAD^"])
                raise

        print(f"\n==> Committed and tagged {tag}")
        print(f"    Push with: git push && git push origin {tag}")
        return 0

    except BumpError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    except subprocess.CalledProcessError as e:
        print(
            f"\nerror: `{' '.join(e.cmd)}` failed with exit status {e.returncode}.\n"
            "       If the failure happened during the build, the release "
            "commit and tag\n"
            "       were reverted and the tree is back at its pre-bump state.",
            file=sys.stderr,
        )
        return e.returncode


if __name__ == "__main__":
    sys.exit(main())
