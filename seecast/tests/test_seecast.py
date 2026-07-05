"""Unit tests for the seecast annotator: transcript rendering (v2 absolute vs v3 relative
timestamps), ANSI stripping, schema validation, the annotate flow with a stubbed model,
and the CLI contract (single-key JSON documents, exit codes, signal discipline).
Run from the repo root with: python3 -m unittest discover -s seecast/tests"""

import contextlib
import importlib.util
import io
import json
import os
import signal
import subprocess
import sys
import tempfile
import unittest
import unittest.mock

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

    def test_watchdog_timeout_is_retried_once_then_fatal(self):
        calls = []

        def flaky(model, prompt, timeout):
            calls.append(1)
            if len(calls) == 1:
                raise RuntimeError("cursor-agent produced no result within 1 s and was killed")
            return '{"title": "T", "summary": "S.", "chapters": [{"t": 0, "title": "A"}]}'

        with tempfile.TemporaryDirectory() as tmp:
            cast = os.path.join(tmp, "rec.cast")
            with open(cast, "w") as f:
                f.write(V3)
            with contextlib.redirect_stderr(io.StringIO()) as err:
                meta = seecast.annotate(cast, run=flaky)
            self.assertEqual(meta["title"], "T")
            self.assertEqual(len(calls), 2, "one retry, no more")
            self.assertIn("retrying once", err.getvalue(), "the retry is announced, not hidden")

            def always_dead(model, prompt, timeout):
                raise RuntimeError("cursor-agent produced no result within 1 s and was killed")

            with contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(RuntimeError, msg="the second failure is final"):
                    seecast.annotate(cast, run=always_dead)


class SchemaMirror(unittest.TestCase):
    """`validate_meta` is a hand-written mirror of the Rust types (the §1 source of truth;
    the schema file is codegen'd from them by `beecast schema`). This cross-check pins the
    facts the mirror relies on, so drift dies in this gate rather than at annotate time."""

    def test_generated_schema_agrees_with_the_mirror(self):
        path = os.path.join(os.path.dirname(__file__), "..", "..", "schema", "beecast-meta.schema.json")
        with open(path, encoding="utf-8") as f:
            schema = json.load(f)
        self.assertIs(schema["additionalProperties"], False, "unknown top-level keys are rejected")
        self.assertEqual(set(schema["properties"]), {"title", "summary", "chapters"})
        chapter = schema["properties"]["chapters"]["items"]
        self.assertIs(chapter["additionalProperties"], False, "unknown chapter keys are rejected")
        self.assertEqual(set(chapter["required"]), {"t", "title"})
        self.assertEqual(chapter["properties"]["t"]["minimum"], 0.0)


class CliContract(unittest.TestCase):
    """The §2 machine-mode contract. `io.StringIO` is not a TTY, so redirecting stdout puts
    `main` in machine mode; the SIGPIPE path needs a real process and gets a subprocess."""

    def run_main(self, argv):
        """Run main(argv) capturing both streams; returns (exit_code, stdout, stderr)."""
        out, err, code = io.StringIO(), io.StringIO(), 0
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            try:
                seecast.main(argv)
            except SystemExit as e:
                code = e.code or 0
        return code, out.getvalue(), err.getvalue()

    def test_version_is_a_single_key_document_and_offline(self):
        code, out, err = self.run_main(["--version"])
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(out), {"Version": {"version": seecast.VERSION}})
        self.assertEqual(err, "", "machine-mode stderr stays quiet")

    def test_help_exitcodes_prints_the_table(self):
        code, out, err = self.run_main(["help", "exitcodes"])
        self.assertEqual(code, 0)
        for token in ("0", "1", "2", "130", "stage", "SIGPIPE"):
            self.assertIn(token, out, "exit-code table must mention %r" % token)
        # A bare `help` prints full usage; an unknown topic is a usage error.
        self.assertEqual(self.run_main(["help"])[0], 0)
        self.assertEqual(self.run_main(["help", "bogus"])[0], 2)

    def test_json_and_color_flags_are_accepted(self):
        # `--json` forces machine mode (redundant here, since a captured stdout already is);
        # `--color=never` parses and cannot break machine output.
        code, out, err = self.run_main(["--json", "--color=never", "--version"])
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(out), {"Version": {"version": seecast.VERSION}})

    def test_usage_error_is_stage_usage_json_with_exit_2(self):
        code, out, err = self.run_main([])
        self.assertEqual(code, 2)
        doc = json.loads(out)
        self.assertIn("required", doc["Error"]["message"])
        self.assertEqual(doc["Error"]["stage"], "usage")

    def test_request_error_is_stage_request_json_with_exit_1(self):
        code, out, err = self.run_main(["--validate", os.path.join(tempfile.gettempdir(), "no-such-file.json")])
        self.assertEqual(code, 1)
        self.assertEqual(json.loads(out)["Error"]["stage"], "request")

    def test_validate_success_is_a_valid_document(self):
        with tempfile.TemporaryDirectory() as tmp:
            sidecar = os.path.join(tmp, "ok.meta.json")
            with open(sidecar, "w") as f:
                json.dump({"title": "T", "chapters": [{"t": 0, "title": "A"}]}, f)
            code, out, err = self.run_main(["--validate", sidecar])
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(out), {"Valid": {"path": sidecar}})

    def test_annotate_wraps_the_write_in_an_annotated_document(self):
        reply = '{"title": "T", "summary": "S.", "chapters": [{"t": 0, "title": "A"}]}'
        with tempfile.TemporaryDirectory() as tmp:
            cast = os.path.join(tmp, "rec.cast")
            with open(cast, "w") as f:
                f.write(V3)
            stub = lambda path, model, timeout: seecast.validate_meta(json.loads(reply), generated=True)
            with unittest.mock.patch.object(seecast, "annotate", stub):
                code, out, err = self.run_main([cast])
            doc = json.loads(out)
            self.assertEqual(code, 0)
            self.assertEqual(doc["Annotated"]["output"], os.path.join(tmp, "rec.meta.json"))
            self.assertEqual(doc["Annotated"]["chapters"], 1)
            self.assertEqual(doc["Annotated"]["meta"]["title"], "T")
            with open(doc["Annotated"]["output"]) as f:
                self.assertEqual(json.load(f)["title"], "T", "the sidecar landed on disk")
            self.assertEqual(err, "", "machine-mode stderr stays quiet")

    def test_dash_output_streams_the_bare_sidecar(self):
        reply = {"title": "T", "summary": "S.", "chapters": [{"t": 0.0, "title": "A"}]}
        with tempfile.TemporaryDirectory() as tmp:
            cast = os.path.join(tmp, "rec.cast")
            with open(cast, "w") as f:
                f.write(V3)
            with unittest.mock.patch.object(seecast, "annotate", lambda *a, **k: reply):
                code, out, err = self.run_main([cast, "-o", "-"])
            self.assertEqual(json.loads(out), reply, "explicit stream mode: no envelope")
            self.assertFalse(os.path.exists(os.path.join(tmp, "rec.meta.json")), "stdout only")

    def test_keyboard_interrupt_exits_130_without_a_traceback(self):
        with unittest.mock.patch.object(seecast, "main", side_effect=KeyboardInterrupt):
            with self.assertRaises(SystemExit) as ctx:
                seecast.entry()
        self.assertEqual(ctx.exception.code, 130)

    @unittest.skipUnless(hasattr(signal, "SIGPIPE"), "SIGPIPE is a Unix concept")
    def test_broken_pipe_dies_quietly(self):
        # The read end of the child's stdout pipe is closed before it ever writes, so its
        # first write raises SIGPIPE deterministically: a quiet signal death, no traceback.
        with tempfile.TemporaryDirectory() as tmp:
            cast = os.path.join(tmp, "rec.cast")
            with open(cast, "w") as f:
                f.write(V2)
            read_end, write_end = os.pipe()
            os.close(read_end)
            proc = subprocess.Popen(
                [sys.executable, os.path.abspath(SCRIPT), "--transcript", cast],
                stdout=write_end,
                stderr=subprocess.PIPE,
            )
            os.close(write_end)
            _, err = proc.communicate()
        self.assertEqual(proc.returncode, -signal.SIGPIPE, "killed by SIGPIPE, not a Python error")
        self.assertNotIn(b"Traceback", err)


if __name__ == "__main__":
    unittest.main()
