#!/usr/bin/env python3
"""
Rewrite homebrew/Formula/tyrannus.rb release tag + per-platform SHA256 placeholders
(HOMEBREW_BUMP_*) using tarballs uploaded for the given Git tag.
"""
from __future__ import annotations

import argparse
import hashlib
import re
import sys
import urllib.error
import urllib.request


def digest_url(url: str) -> str:
    digest = hashlib.sha256()
    with urllib.request.urlopen(url, timeout=300) as r:
        while True:
            chunk = r.read(65536)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("release_tag", help="GitHub release tag (e.g. 0.1.2 or v0.1.2)")
    p.add_argument(
        "--formula",
        default="homebrew/Formula/tyrannus.rb",
        help="Path to tyrannus.rb",
    )
    p.add_argument(
        "--repo",
        default="huffs-projects/tyrannus",
        help="OWNER/NAME for downloads",
    )
    args = p.parse_args()
    # Must match GitHub tag verbatim (URLs and archive names use RELEASE_TAG).
    release_tag = args.release_tag.strip()
    brew_version = release_tag[1:] if release_tag.startswith("v") else release_tag
    slug = lambda target: (
        f"https://github.com/{args.repo}/releases/download/"
        f"{release_tag}/tyrannus-{release_tag}-{target}.tar.gz"
    )
    mac_url = slug("macos-aarch64")
    lin_url = slug("linux-x86_64")
    try:
        mac_sha = digest_url(mac_url)
        lin_sha = digest_url(lin_url)
    except urllib.error.HTTPError as e:
        print(f"bump-homebrew: failed to download release assets: {e}", file=sys.stderr)
        return 1

    path = args.formula
    text = open(path, encoding="utf-8").read()

    def sub_once(pattern: str, repl: str, label: str) -> str:
        new, n = re.subn(pattern, repl, text, count=1, flags=re.MULTILINE)
        if n != 1:
            raise SystemExit(f"bump-homebrew: expected one {label} match in {path}, got {n}")
        return new

    text = sub_once(
        r'^(\s*)RELEASE_TAG = "[^"]*"\.freeze(\s*# HOMEBREW_BUMP_TAG\s*)$',
        rf'\1RELEASE_TAG = "{release_tag}".freeze\2',
        "HOMEBREW_BUMP_TAG",
    )
    text = sub_once(
        r'^(\s*)version "[^"]*"(\s*# HOMEBREW_BUMP_VERSION\s*)$',
        rf'\1version "{brew_version}"\2',
        "HOMEBREW_BUMP_VERSION",
    )
    text = sub_once(
        r'^(\s*)sha256 "[0-9a-f]{64}"(\s*# HOMEBREW_BUMP_MACOS_AARCH64\s*)$',
        rf'\1sha256 "{mac_sha}"\2',
        "HOMEBREW_BUMP_MACOS_AARCH64",
    )
    text = sub_once(
        r'^(\s*)sha256 "[0-9a-f]{64}"(\s*# HOMEBREW_BUMP_LINUX_X86_64\s*)$',
        rf'\1sha256 "{lin_sha}"\2',
        "HOMEBREW_BUMP_LINUX_X86_64",
    )

    open(path, "w", encoding="utf-8").write(text)
    print(f"bump-homebrew: updated {path} for tag {release_tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
