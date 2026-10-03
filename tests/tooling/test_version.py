import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("maintain", Path(__file__).resolve().parents[2] / "scripts/maintain.py")
maintain = importlib.util.module_from_spec(spec)
spec.loader.exec_module(maintain)


class AutomaticVersionTests(unittest.TestCase):
    def test_priority(self):
        self.assertEqual(maintain.choose_bump(["fix(repo): hash check", "feat(ui): search"]), "minor")
        self.assertEqual(maintain.choose_bump(["feat!: remove old API"]), "major")
        self.assertEqual(maintain.choose_bump(["fix(repo): change\n\nBREAKING CHANGE: archive layout"]), "major")
        self.assertEqual(maintain.choose_bump(["docs: clarify setup"]), "patch")

    def test_ambiguous_history_fails(self):
        with self.assertRaises(ValueError):
            maintain.choose_bump(["misc updates"])
        with self.assertRaises(ValueError):
            maintain.choose_bump([])
