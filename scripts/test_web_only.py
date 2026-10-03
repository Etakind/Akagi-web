#!/usr/bin/env python3
"""Offline source-boundary and target-inventory regressions."""
import json
import pathlib
import unittest
ROOT = pathlib.Path(__file__).resolve().parents[1]
class WebOnlyTests(unittest.TestCase):
    def test_target_inventory(self):
        targets = json.loads((ROOT / 'build/targets.json').read_text())
        self.assertEqual(len(targets), 5)
        self.assertEqual(len({t['target'] for t in targets}), 5)
        for target in targets:
            self.assertIn('zip', target['formats'])
    def test_removed_runtime_stays_removed(self):
        for path in ['src/proxy/mod.rs', 'src/bot/api.rs', 'src/bot/install.rs', 'src/bot/runtime.rs', 'src/updater/apply.rs', 'scripts/fetch-runtime.sh']:
            self.assertFalse((ROOT / path).exists(), path)
        for game in ['majsoul', 'tenhou']:
            self.assertTrue((ROOT / 'src/bridge' / game / 'mod.rs').exists())
if __name__ == '__main__': unittest.main()
