import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch
import tempfile

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

    def test_changelog_promotes_only_unreleased_changes(self):
        text = '# Changelog\n\n## Unreleased\n\n- New feature.\n\n## 0.1.0\n\n- Original release.\n'
        updated = maintain.promote_changelog(text, '0.2.0')
        self.assertIn('## Unreleased\n\n## 0.2.0', updated)
        self.assertTrue(updated.endswith('## 0.1.0\n\n- Original release.\n'))
        self.assertEqual(maintain.release_notes(updated, '0.2.0'), '- New feature.\n')
        with self.assertRaises(AssertionError):
            maintain.promote_changelog(updated, '0.3.0')
        with self.assertRaises(AssertionError):
            maintain.release_notes(updated, '0.3.0')

    def test_release_ignores_other_versions_and_packaging_revisions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'debian').mkdir()
            (root / 'dist').mkdir()
            (root / 'debian/changelog').write_text('xxc-aptd (0.2.0-2) unstable; urgency=medium\n')
            for name in ['xxc-aptd_0.1.0-1_amd64.deb', 'xxc-aptd_0.2.0-1_amd64.deb', 'xxc-aptd_0.2.0-2_amd64.deb']:
                (root / 'dist' / name).touch()
            with patch.object(maintain, 'ROOT', root), patch.object(maintain, 'capture', side_effect=['xxc-aptd', '0.2.0-2']):
                self.assertEqual([p.name for p in maintain.release_artifacts()], ['xxc-aptd_0.2.0-2_amd64.deb'])
            with patch.object(maintain, 'ROOT', root), patch.object(maintain, 'capture', return_value='wrong-package'):
                with self.assertRaises(AssertionError):
                    maintain.release_artifacts()
