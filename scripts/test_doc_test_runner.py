#!/usr/bin/env python3
"""Regression checks for typechecking literal documentation with the default compiler."""
from pathlib import Path
import tempfile
import unittest

import run_doc_tests as runner

ROOT = Path(__file__).resolve().parents[1]
FERN = str(ROOT / "bin/fern")


class DocumentationRunnerTests(unittest.TestCase):
    """Exercise the real Rust checker without executing documented filesystem/network calls."""

    def check(self, code: str) -> bool:
        """Validate one snippet through the same direct/wrapped path as the CLI."""
        snippet = runner.DocSnippet(Path("example.fn"), 1, code)
        passed, output = runner.check_snippet(snippet, FERN)
        self.last_output = output
        return passed

    def test_expression_wrapper_has_valid_module_identity(self):
        self.assertTrue(self.check('println("hello")'), self.last_output)

    def test_indented_control_flow_is_preserved(self):
        code = 'let value = if true:\n    1\nelse:\n    2\nprintln(value)'
        self.assertTrue(self.check(code), self.last_output)

    def test_expectation_like_string_contents_are_not_rewritten(self):
        self.assertTrue(self.check('println("literal # => text")'), self.last_output)

    def test_fallible_calls_must_be_handled(self):
        self.assertFalse(self.check('fs.read("missing.txt")'))
        self.assertIn("Result", self.last_output)
        self.assertTrue(self.check('match fs.read("missing.txt"):\n    Ok(text) -> println(text)\n    Err(_) -> println("missing")'), self.last_output)

    def test_full_program_module_names_match_the_temporary_path(self):
        self.assertTrue(self.check('module samples.example\nfn main(): println("hello")'), self.last_output)

    def test_module_paths_cannot_escape_the_temporary_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            for module in ("../escape", "a/b", "a\\b", ".hidden"):
                with self.assertRaises(ValueError):
                    runner.temporary_source_path(Path(temp), f"module {module}\nfn main(): ()")
            self.assertEqual(list(Path(temp).iterdir()), [])

    def test_real_type_errors_are_not_hidden_by_wrapping(self):
        self.assertFalse(self.check('let value: Int = "wrong"\nprintln(value)'))
        self.assertFalse(self.check('fn main(): unknown_name()'))

    def test_extraction_retains_common_indentation_and_source_lines(self):
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / "source.fn"
            source.write_text('@doc """\nExample:\n```fern\n    if true:\n        println("ok")\n```\n"""\nfn main(): ()\n')
            snippets = runner.extract_snippets(source)
        self.assertEqual(len(snippets), 1)
        self.assertEqual(snippets[0].line, 4)
        self.assertEqual(snippets[0].code, 'if true:\n    println("ok")')


if __name__ == "__main__":
    unittest.main()
