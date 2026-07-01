#!/usr/bin/env python3
"""Bump Gospel's canonical Cargo version and sync derived metadata."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CARGO_TOML = ROOT / "src-tauri" / "Cargo.toml"
SYNC_SCRIPT = ROOT / "scripts" / "sync-version.py"
SEMVER_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")
VERSION_LINE_RE = re.compile(r'(?m)^version\s*=\s*"([^"]+)"\s*$')


def read_cargo_toml() -> str:
    return CARGO_TOML.read_text(encoding="utf-8")


def read_cargo_version(cargo_toml: str) -> str:
    package_match = re.search(r"(?ms)^\[package\]\s*(.*?)(?=^\[|\Z)", cargo_toml)
    if package_match is None:
        raise ValueError(f"Could not find [package] section in {CARGO_TOML}")

    version_match = VERSION_LINE_RE.search(package_match.group(1))
    if version_match is None:
        raise ValueError(f"Could not find package version in {CARGO_TOML}")

    return version_match.group(1)


def parse_version(version: str) -> tuple[int, int, int]:
    match = SEMVER_RE.fullmatch(version)
    if match is None:
        raise ValueError(f"Expected clean SemVer major.minor.patch, got {version!r}")

    return tuple(int(part) for part in match.groups())


def bump_version(version: str, part: str) -> str:
    major, minor, patch = parse_version(version)

    if part == "major":
        return f"{major + 1}.0.0"
    if part == "minor":
        return f"{major}.{minor + 1}.0"
    if part == "patch":
        return f"{major}.{minor}.{patch + 1}"

    raise ValueError(f"Unknown bump part {part!r}")


def replace_cargo_version(cargo_toml: str, old_version: str, new_version: str) -> str:
    package_match = re.search(r"(?ms)^\[package\]\s*(.*?)(?=^\[|\Z)", cargo_toml)
    if package_match is None:
        raise ValueError(f"Could not find [package] section in {CARGO_TOML}")

    package_body = package_match.group(1)
    next_package_body, replacements = VERSION_LINE_RE.subn(
        f'version = "{new_version}"',
        package_body,
        count=1,
    )
    if replacements != 1:
        raise ValueError(f"Could not replace package version {old_version!r}")

    start, end = package_match.span(1)
    return cargo_toml[:start] + next_package_body + cargo_toml[end:]


def sync_release_version() -> None:
    subprocess.run(
        [sys.executable, str(SYNC_SCRIPT), "--release"],
        cwd=ROOT,
        check=True,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Bump src-tauri/Cargo.toml and sync package metadata",
    )
    parser.add_argument(
        "part",
        nargs="?",
        choices=("patch", "minor", "major"),
        default="patch",
        help="SemVer part to bump (default: patch)",
    )
    parser.add_argument(
        "--set",
        dest="set_version",
        metavar="VERSION",
        help="set an explicit clean SemVer instead of incrementing",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print the next version without writing files",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    try:
        cargo_toml = read_cargo_toml()
        current_version = read_cargo_version(cargo_toml)
        new_version = args.set_version or bump_version(current_version, args.part)
        parse_version(new_version)

        if args.dry_run:
            print(f"{current_version} -> {new_version}")
            return 0

        if current_version == new_version:
            print(f"Version already {new_version}")
            return 0

        CARGO_TOML.write_text(
            replace_cargo_version(cargo_toml, current_version, new_version),
            encoding="utf-8",
        )
        sync_release_version()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"bump-version: {error}", file=sys.stderr)
        return 1

    print(f"Bumped version {current_version} -> {new_version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
