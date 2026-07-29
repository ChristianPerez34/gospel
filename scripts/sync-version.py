#!/usr/bin/env python3
"""Sync Gospel's app version from src-tauri/Cargo.toml."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CARGO_TOML = ROOT / "src-tauri" / "Cargo.toml"
CARGO_LOCK = ROOT / "src-tauri" / "Cargo.lock"
PACKAGE_JSON = ROOT / "package.json"
TAURI_CONFIG = ROOT / "src-tauri" / "tauri.conf.json"


def read_cargo_version(path: Path = CARGO_TOML) -> str:
    cargo_toml = path.read_text(encoding="utf-8")
    package_match = re.search(r"(?ms)^\[package\]\s*(.*?)(?=^\[|\Z)", cargo_toml)
    if package_match is None:
        raise ValueError(f"Could not find [package] section in {path}")

    version_match = re.search(
        r'(?m)^version\s*=\s*"([^"]+)"\s*$',
        package_match.group(1),
    )
    if version_match is None:
        raise ValueError(f"Could not find package version in {path}")

    return version_match.group(1)


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def read_cargo_lock_version(path: Path = CARGO_LOCK) -> str:
    cargo_lock = path.read_text(encoding="utf-8")
    for package_match in re.finditer(
        r"(?ms)^\[\[package\]\]\s*(.*?)(?=^\[\[package\]\]|\Z)",
        cargo_lock,
    ):
        package = package_match.group(1)
        if re.search(r'(?m)^name\s*=\s*"gospel"\s*$', package):
            version_match = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', package)
            if version_match is not None:
                return version_match.group(1)

    raise ValueError(f"Could not find gospel package version in {path}")


def write_json(path: Path, data: dict[str, Any]) -> None:
    path.write_text(
        json.dumps(data, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )


def sync_json_version(path: Path, version: str) -> bool:
    data = read_json(path)
    if data.get("version") == version:
        return False

    data["version"] = version
    write_json(path, data)
    return True


def target_version(base_version: str, dev: bool) -> str:
    if dev:
        if base_version.endswith("-dev"):
            raise ValueError("Cargo.toml must not use the reserved -dev suffix")
        return f"{base_version}-dev"

    return base_version


def check_release_versions(version: str, tag: str | None) -> None:
    versions = {
        CARGO_LOCK: read_cargo_lock_version(),
        PACKAGE_JSON: read_json(PACKAGE_JSON).get("version"),
        TAURI_CONFIG: read_json(TAURI_CONFIG).get("version"),
    }
    mismatches = [
        f"{path.relative_to(ROOT)} has {actual!r}"
        for path, actual in versions.items()
        if actual != version
    ]
    if mismatches:
        details = ", ".join(mismatches)
        raise ValueError(f"Expected release version {version!r}; {details}")

    expected_tag = f"v{version}"
    if tag is not None and tag != expected_tag:
        raise ValueError(f"Expected tag {expected_tag!r}, got {tag!r}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Sync package.json and tauri.conf.json from src-tauri/Cargo.toml",
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--dev",
        action="store_true",
        help="append -dev to derived package metadata",
    )
    mode.add_argument(
        "--release",
        action="store_true",
        help="sync the clean Cargo.toml version",
    )
    mode.add_argument(
        "--check",
        action="store_true",
        help="verify release metadata without modifying files",
    )
    parser.add_argument(
        "--tag",
        help="also require the supplied tag to equal v plus the Cargo version",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    if args.tag is not None and not args.check:
        print("sync-version: --tag requires --check", file=sys.stderr)
        return 2

    try:
        base_version = read_cargo_version()
        if args.check:
            check_release_versions(base_version, args.tag)
            tag_detail = f" and tag {args.tag}" if args.tag is not None else ""
            print(f"Verified release version {base_version}{tag_detail}")
            return 0

        version = target_version(base_version, dev=args.dev)
        changed = [
            path
            for path in (PACKAGE_JSON, TAURI_CONFIG)
            if sync_json_version(path, version)
        ]
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"sync-version: {error}", file=sys.stderr)
        return 1

    changed_list = ", ".join(str(path.relative_to(ROOT)) for path in changed) or "no files"
    print(f"Synced version {version} ({changed_list})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
