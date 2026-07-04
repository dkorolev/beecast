"""Style gate: the portable rules from the repo's `rustfmt.toml` (`max_width = 120`,
`hard_tabs = false`), applied to the repo's Python — lines at most 120 columns, no hard tabs.
The rest of rustfmt is Rust-specific (and enforced by `cargo fmt --check`); indentation
stays Python-conventional 4 spaces. Stdlib-only on purpose, so the existing gate
(`python3 -m unittest discover -s seecast/tests`) enforces style with no new tooling."""

import pathlib
import unittest

REPO = pathlib.Path(__file__).resolve().parent.parent.parent
MAX_WIDTH = 120


class Style(unittest.TestCase):
    def test_max_width_and_no_hard_tabs(self):
        checked = 0
        for path in sorted(REPO.rglob("*.py")):
            if ".git" in path.parts or "target" in path.parts or "tmp" in path.parts:
                continue
            checked += 1
            for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                where = "%s:%d" % (path.relative_to(REPO), lineno)
                self.assertLessEqual(len(line), MAX_WIDTH, "%s is over %d columns" % (where, MAX_WIDTH))
                self.assertNotIn("\t", line, "%s contains a hard tab" % where)
        self.assertGreater(checked, 1, "style gate found no Python files - the rglob is broken")


if __name__ == "__main__":
    unittest.main()
