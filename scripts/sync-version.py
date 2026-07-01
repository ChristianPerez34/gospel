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
            raise ValueError("Cargo.toml must store a clean version without a -dev suffix")
        return f"{base_version}-dev"

    return base_version


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
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    try:
        base_version = read_cargo_version()
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
