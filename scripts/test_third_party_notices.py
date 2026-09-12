#!/usr/bin/env python3
"""Pin release notices to the enabled locked Unix dependency graph and native sources."""
import json
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parent.parent
TARGETS = ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
           "x86_64-apple-darwin", "aarch64-apple-darwin")


def packages(target):
    """Resolve enabled normal/build dependencies without fetching or compiling anything."""
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1",
         "--manifest-path", str(ROOT / "compiler-rs/Cargo.toml"), "--filter-platform", target],
        check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=60)
    metadata = json.loads(result.stdout)
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    pending = [metadata["resolve"]["root"]]
    enabled = set()
    while pending:
        identifier = pending.pop()
        if identifier in enabled:
            continue
        enabled.add(identifier)
        pending.extend(dep["pkg"] for dep in nodes[identifier]["deps"]
                       if any(kind["kind"] != "dev" for kind in dep["dep_kinds"]))
    return [package for package in metadata["packages"]
            if package["id"] in enabled and package["source"] is not None]


class ThirdPartyNotices(unittest.TestCase):
    def test_each_default_unix_dependency_has_its_exact_notice_and_license(self):
        notices = (ROOT / "THIRD_PARTY_NOTICES.md").read_text()
        for target in TARGETS:
            for package in packages(target):
                with self.subTest(target=target, package=package["name"]):
                    self.assertIn(f"### `{package['name']}` {package['version']}\n", notices)
                    self.assertIn("MIT", package["license"].split(" OR "))
                    directory = Path(package["manifest_path"]).parent
                    license_path = directory / "LICENSE-MIT"
                    if not license_path.exists():
                        license_path = directory / "LICENSE"
                    self.assertIn(license_path.read_text().strip(), notices)
                    additional = directory / "NOTICES.md"
                    if additional.exists():
                        self.assertIn(additional.read_text().strip(), notices)

    def test_native_notices_are_reproduced_from_distributed_sources(self):
        notices = (ROOT / "THIRD_PARTY_NOTICES.md").read_text()
        for path in ("deps/qbe/LICENSE", "deps/unicode/LICENSE.txt"):
            self.assertIn((ROOT / path).read_text().strip(), notices)
        civetweb = (ROOT / "deps/civetweb/LICENSE.md").read_text()
        section = civetweb.split("Civetweb License", 1)[1].split("Lua License", 1)[0]
        text = "\n".join(line.removeprefix("> ").removeprefix(">")
                         for line in section.splitlines() if line.startswith(">"))
        self.assertIn(text.strip(), notices)
        self.assertIn("Copyright (c) 2010-2023, Salvatore Sanfilippo", notices)
        self.assertIn("Copyright (C) 1999, 2000, 2002 Aladdin Enterprises", notices)


if __name__ == "__main__":
    unittest.main()
