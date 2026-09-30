#!/usr/bin/env python3
"""Publish a complete update manifest only after every platform artifact exists."""
import argparse
import hashlib
import json
from pathlib import Path
import re

ASSETS = (
    "Sidedoor-macos-arm64.zip",
    "Sidedoor-macos-arm64.dmg",
    "Sidedoor-windows-x64.zip",
    "Sidedoor-windows-x64.msi",
    "Sidedoor-windows-x64-setup.exe",
    "Sidedoor-linux-x86_64.tar.gz",
    "Sidedoor-linux-x86_64.deb",
    "Sidedoor-linux-x86_64.rpm",
)


def manifest(directory: Path, version: str) -> dict:
    version = version.removeprefix("v")
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version):
        raise ValueError("Expected a semantic release version")
    assets = []
    for name in ASSETS:
        package = directory / name
        size = package.stat().st_size
        if not 0 < size <= 1024 * 1024 * 1024:
            raise ValueError(f"Invalid payload size: {name}")
        with package.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        checksum = (directory / (name + ".sha256")).read_text().strip().split()
        if len(checksum) != 2 or checksum[0].lower() != digest or checksum[1].lstrip("*") != name:
            raise ValueError(f"Checksum mismatch: {name}")
        assets.append({"name": name, "size": size, "sha256": digest})
    return {"schema_version": 1, "version": version, "assets": assets}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("version")
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    result = manifest(args.directory, args.version)
    output = args.directory / "sidedoor-update.json"
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(f"Created {output} for {result['version']}")
