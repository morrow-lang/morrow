#!/usr/bin/env python3
"""Default release workflows preserve executable bundles and synchronized versions."""
import json
from pathlib import Path
import re
import tomllib
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


class DefaultCiWorkflow(unittest.TestCase):
    def test_release_automation_updates_both_compilers_and_path_dependency_locks(self):
        config = json.loads((ROOT / ".github/release-please-config.json").read_text())
        extra = {entry["path"]: entry for entry in config["packages"]["."]["extra-files"]}
        # release-please GenericToml filters its tagged scalar tree: names live
        # under .value, while the selected version stays a replaceable TOML node.
        expected = {
            "compiler-rs/Cargo.toml": "$.package.version",
            "compiler-rs/Cargo.lock": "$.package[?(@.name.value == 'fern-prototype')].version",
            "benchmarks/compiler-phases/Cargo.lock": "$.package[?(@.name.value == 'fern-prototype')].version",
        }
        self.assertEqual(extra["include/version.h"]["type"], "generic")
        for path, selector in expected.items():
            with self.subTest(path=path):
                self.assertEqual(extra.get(path), {"type": "toml", "path": path, "jsonpath": selector})
        version = tomllib.loads((ROOT / "compiler-rs/Cargo.toml").read_text())["package"]["version"]
        c_version = re.search(r'^#define FERN_VERSION_STRING "([^"]+)"',
                              (ROOT / "include/version.h").read_text(), re.MULTILINE).group(1)
        self.assertEqual(version, c_version)
        for path in ("compiler-rs/Cargo.lock", "benchmarks/compiler-phases/Cargo.lock"):
            records = tomllib.loads((ROOT / path).read_text())["package"]
            versions = [record["version"] for record in records if record["name"] == "fern-prototype"]
            self.assertEqual(versions, [version])

    def test_memory_report_measures_the_default_compiler_it_labels(self):
        import compare_memory_paths
        output = "binary_size_bytes=3145728\nstartup_ms median=3.0 p95=4.0 max=5.0\n"
        class Result:
            stdout = output
        with patch.object(compare_memory_paths, "run", return_value=Result()) as run:
            result = compare_memory_paths.collect_perf_snapshot(3)
        command = run.call_args.args[0]
        self.assertIn("--bin-path", command)
        self.assertEqual(command[command.index("--bin-path") + 1], "bin/fern")
        self.assertEqual(command[command.index("--binary-budget-bytes") + 1], "4194304")
        self.assertEqual(result.binary_size_bytes, 3145728)

    def test_readiness_upload_is_a_complete_permission_preserving_bundle(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        readiness = workflow.split("  release-readiness:", 1)[1].split("  package-bundles:", 1)[0]
        package = readiness.find("mise run --skip-deps release-package")
        upload = readiness.find("name: Upload readiness bundle artifact")
        self.assertGreaterEqual(package, 0)
        self.assertGreater(upload, package)
        artifact = readiness[upload:].split("      - name:", 1)[0]
        self.assertIn("dist/*.tar.gz", artifact)
        self.assertIn("dist/*.tar.gz.sha256", artifact)
        self.assertIn("if-no-files-found: error", artifact)
        self.assertNotIn("path: bin/fern", readiness)

    def test_ci_checks_its_default_workflow_contract_and_current_notices(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("python3 scripts/test_default_ci_workflow.py", workflow)
        release = (ROOT / ".github/workflows/release.yml").read_text()
        package = release.split("  package-bundles:", 1)[1]
        self.assertIn("python3 scripts/test_third_party_notices.py", package)
        self.assertLess(package.index("python3 scripts/test_third_party_notices.py"),
                        package.index("name: Upload packaged artifacts"))


if __name__ == "__main__":
    unittest.main()
