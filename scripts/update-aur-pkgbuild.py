#!/usr/bin/env python3
"""
Write tyrannus PKGBUILD for the AUR from contrib/aur/tyrannus.PKGBUILD.in.

Fetches linux-x86_64 tarball SHA256 from GitHub Releases (same URL layout as Homebrew bump).
Requires an existing Release with assets for release_tag.

Default github repo matches Formula: huffs-projects/tyrannus.
"""
from __future__ import annotations

import argparse
import hashlib
import pathlib
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
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "release_tag",
        help="GitHub release tag (verbatim; used in tarball URL basename)",
    )
    p.add_argument(
        "--repo",
        default="huffs-projects/tyrannus",
        help="OWNER/NAME on GitHub",
    )
    p.add_argument(
        "--template",
        default="contrib/aur/tyrannus.PKGBUILD.in",
        help="PKGBUILD template path (cwd = repo root)",
    )
    p.add_argument(
        "--output",
        required=True,
        help="Destination PKGBUILD path",
    )
    args = p.parse_args()

    release_tag = args.release_tag.strip()
    pkgver = release_tag[1:] if release_tag.startswith("v") else release_tag
    tarball_url = (
        f"https://github.com/{args.repo}/releases/download/"
        f"{release_tag}/tyrannus-{release_tag}-linux-x86_64.tar.gz"
    )
    try:
        sha = digest_url(tarball_url)
    except urllib.error.HTTPError as e:
        print(f"update-aur-pkgbuild: failed to fetch release archive: {e}", file=sys.stderr)
        return 1

    home = f"https://github.com/{args.repo}"

    tmpl_path = pathlib.Path(args.template)
    text = tmpl_path.read_text(encoding="utf-8")
    for token, repl in (
        ("__PKGVER__", pkgver),
        ("__HOMEPAGE__", home),
        ("__RELEASE_TAG__", release_tag),
        ("__SHA256__", sha),
        ("__REPO__", args.repo),
    ):
        if token not in text:
            raise SystemExit(f"update-aur-pkgbuild: missing {token} in {tmpl_path}")
        text = text.replace(token, repl)

    out = pathlib.Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(text, encoding="utf-8")
    print(f"update-aur-pkgbuild: wrote {out} for tag {release_tag} (pkgver {pkgver})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
