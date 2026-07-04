"""Unit tests for the seecast annotator: transcript rendering (v2 absolute vs v3 relative
timestamps), ANSI stripping, schema validation, and the annotate flow with a stubbed model.
Run with: python3 -m unittest discover -s tests"""

import importlib.util
import json
import os
import sys
import tempfile
import unittest

SCRIPT = os.path.join(os.path.dirname(__file__), "..", "..", ".skills", "seecast", "scripts", "seecast.py")
spec = importlib.util.spec_from_file_location("seecast", SCRIPT)
seecast = importlib.util.module_from_spec(spec)
spec.loader.exec_module(seecast)

V2 = (
    '{"version":2,"width":80,"height":24}\n'
    '[0.5,"o","\\u001b[2Jhello\\r\\n"]\n[1.0,"o","hello\\r\\n"]\n[65.0,"o","done\\r\\n"]\n'
)
V3 = (
    '{"version":3,"term":{"cols":80,"rows":24}}\n'
    '[0.5,"o","hello\\r\\n"]\n[1.0,"o","world\\r\\n"]\n[63.5,"o","done\\r\\n"]\n'
)


class StripAnsi(unittest.TestCase):
    def test_removes_csi_osc_and_control(self):
        self.assertEqual(seecast.strip_ansi("\x1b[31mred\x1b[0m"), "red")
        self.assertEqual(seecast.strip_ansi("\x1b]0;title\x07text"), "text")
        self.assertEqual(seecast.strip_ansi("a\x1b[Kb"), "ab")
        self.assertEqual(seecast.strip_ansi("line1\r\nline2"), "line1\n\nline2")


class Transcript(unittest.TestCase):
    def test_v2_uses_absolute_times_and_dedups(self):
        t = seecast.transcript(V2)
        self.assertIn("[0.5s] hello", t)
        self.assertNotIn("[1.0s] hello", t, "consecutive duplicate dropped")
        self.assertIn("[65.0s] done", t)

    def test_v3_accumulates_relative_intervals(self):
        t = seecast.transcript(V3)
        self.assertIn("[0.5s] hello", t)
        self.assertIn("[1.5s] world", t)  # 0.5 + 1.0
        self.assertIn("[65.0s] done", t)  # 0.5 + 1.0 + 63.5

    def test_downsamples_to_max_lines(self):
        cast = '{"version":2,"width":80,"height":24}\n' + "".join(
            '[%d,"o","line %d\\r\\n"]\n' % (i, i) for i in range(1, 400)
        )
        t = seecast.transcript(cast, max_lines=50)
        self.assertLessEqual(len(t.splitlines()), 50)
        self.assertIn("[1.0s] line 1", t, "chronological order preserved")

    def test_drops_spinner_noise(self):
        cast = (
            '{"version":2,"width":80,"height":24}\n'
            '[1,"o","✶\\r\\n"]\n[2,"o","i…\\r\\n"]\n[3,"o","✻3\\r\\n"]\n[4,"o","build passed\\r\\n"]\n'
        )
        t = seecast.transcript(cast)
        self.assertEqual(t, "[4.0s] build passed")

    def test_tolerates_truncated_live_tail(self):
        t = seecast.transcript(V3 + '[1.0,"o","tru')
        self.assertIn("[65.0s] done", t)

    def test_rejects_non_asciicast(self):
        with self.assertRaises(ValueError):
            seecast.transcript("hello world\n")
        with self.assertRaises(ValueError):
            seecast.transcript('{"version":1,"stdout":[]}\n')


class ValidateMeta(unittest.TestCase):
    def test_generated_sorts_and_pins_first_to_zero(self):
        meta = seecast.validate_meta(
            {"title": "T", "summary": "S.", "chapters": [{"t": 8.5, "title": "Finish"}, {"t": 2.3, "title": "Start"}]},
            generated=True,
        )
        self.assertEqual(meta["chapters"][0], {"t": 0.0, "title": "Start"})  # sorted, then pinned
        self.assertEqual(meta["chapters"][1], {"t": 8.5, "title": "Finish"})  # fractional timekey preserved

    def test_generated_requires_all_three_fields(self):
        for missing in ({"summary": "S.", "chapters": [{"t": 0, "title": "A"}]},
                        {"title": "T", "chapters": [{"t": 0, "title": "A"}]},
                        {"title": "T", "summary": "S.", "chapters": []}):
            with self.assertRaises(ValueError):
                seecast.validate_meta(missing, generated=True)

    def test_rejects_unknown_fields_and_bad_shapes(self):
        with self.assertRaises(ValueError):
            seecast.validate_meta({"titel": "typo"})
        with self.assertRaises(ValueError):
            seecast.validate_meta({"chapters": [{"t": 0, "title": "A", "note": "no"}]})
        with self.assertRaises(ValueError):
            seecast.validate_meta({"chapters": [{"t": True, "title": "A"}]})
        with self.assertRaises(ValueError):
            seecast.validate_meta({"chapters": [{"t": -1, "title": "A"}]})
        with self.assertRaises(ValueError):
            seecast.validate_meta({"title": "  "})

    def test_sidecar_validation_keeps_schema_invariants(self):
        # Not generated: fields are optional, but order and the t=0 pin are still law.
        self.assertEqual(seecast.validate_meta({}), {})
        with self.assertRaises(ValueError):
            seecast.validate_meta({"chapters": [{"t": 5, "title": "late"}]})
        with self.assertRaises(ValueError):
            seecast.validate_meta({"chapters": [{"t": 0, "title": "A"}, {"t": 0, "title": "tie"}]})


class ExtractJson(unittest.TestCase):
    def test_unwraps_prose_and_fences(self):
        obj = seecast.extract_json('Sure:\n```json\n{"title": "T"}\n```\ndone')
        self.assertEqual(obj, {"title": "T"})
        with self.assertRaises(ValueError):
            seecast.extract_json("no json here")


class Annotate(unittest.TestCase):
    def test_stubbed_run_produces_validated_meta(self):
        prompts = []

        def stub(model, prompt, timeout):
            prompts.append((model, prompt))
            return '{"title": "Hello run", "summary": "Says hello.", "chapters": [{"t": 0, "title": "Start"}]}'

        with tempfile.TemporaryDirectory() as tmp:
            cast = os.path.join(tmp, "rec.cast")
            with open(cast, "w") as f:
                f.write(V3)
            meta = seecast.annotate(cast, run=stub)
        self.assertEqual(meta["title"], "Hello run")
        self.assertEqual(meta["chapters"], [{"t": 0.0, "title": "Start"}])
        self.assertIn("[0.5s] hello", prompts[0][1], "the transcript is embedded in the prompt")
        self.assertEqual(prompts[0][0], seecast.DEFAULT_MODEL)

    def test_invalid_reply_means_no_sidecar(self):
        def bad_stub(model, prompt, timeout):
            return '{"summary": "no title or chapters"}'

        with tempfile.TemporaryDirectory() as tmp:
            cast = os.path.join(tmp, "rec.cast")
            with open(cast, "w") as f:
                f.write(V2)
            with self.assertRaises(ValueError):
                seecast.annotate(cast, run=bad_stub)


if __name__ == "__main__":
    unittest.main()
