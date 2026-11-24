"""Model-free sample/protocol regressions for phrase dictation in the copy.

All neural boundaries use simulated detectors/sessions. No microphone, Whisper
model or real ONNX session is opened by this suite.
"""
import base64
import io
import json
from pathlib import Path
import struct
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import wave

import numpy as np

import echo_worker as worker
from echo_streaming_vad import FRAME, RATE, StatefulSilero


def pcm(samples, value=0):
    return struct.pack("<h", value) * samples


class Detector:
    def __init__(self, probabilities=None):
        self.probabilities = iter(probabilities) if probabilities is not None else None
        self.calls = 0
        self.last = None

    def probability(self, frame):
        if frame.shape != (FRAME,):
            raise AssertionError("Classifier must receive one full frame")
        self.calls += 1
        self.last = frame.copy()
        if self.probabilities is not None:
            return next(self.probabilities)
        return 0.9 if np.any(frame != 0) else 0.0


class LiveVadTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="echo-continuous-test-")
        self.addCleanup(self.temp.cleanup)
        self.events = []
        self.hook = patch.object(worker, "emit", side_effect=lambda event, **values: self.events.append((event, values)))
        self.hook.start()
        self.addCleanup(self.hook.stop)

    def create(self, detector=None, **overrides):
        request = {"directory": self.temp.name, "id": "session", "silence_ms": 600}
        request.update(overrides)
        return worker.LiveVad(request, detector=detector if detector is not None else Detector())

    def utterances(self):
        return [values for event, values in self.events if event == "utterance"]

    def read(self, utterance):
        with wave.open(utterance["path"], "rb") as wav:
            self.assertEqual((wav.getnchannels(), wav.getsampwidth(), wav.getframerate()), (1, 2, RATE))
            return wav.readframes(wav.getnframes())

    def test_silence_keeps_only_600ms_and_never_writes_a_take(self):
        live = self.create()
        for _ in range(1200):
            live.feed(pcm(FRAME))
        self.assertEqual(live.segmenter.preroll_size, 9600)
        self.assertEqual(sum(len(part) for part in live.segmenter.preroll), 19200)
        self.assertEqual(len(live.pending), 0)
        self.assertEqual(len(live.segmenter.audio), 0)
        live.finish()
        self.assertEqual(self.events, [])
        self.assertEqual(list(Path(self.temp.name).iterdir()), [])

    def test_pause_seals_exact_preroll_and_each_phrase_once(self):
        live = self.create()
        silence, speech, pause = pcm(FRAME * 50), pcm(FRAME * 4, 1234), pcm(FRAME * 19)
        live.feed(silence)
        live.feed(speech)
        self.assertEqual(self.utterances(), [])
        live.feed(pause)
        first = self.utterances()[0]
        self.assertEqual(self.read(first), silence[-9600 * 2:] + speech + pause)
        live.feed(speech)
        live.feed(pause)
        live.finish()
        self.assertEqual(len(self.utterances()), 2)
        self.assertNotEqual(first["id"], self.utterances()[1]["id"])
        self.assertEqual(self.read(self.utterances()[1]), speech + pause)

    def test_chunk_boundaries_and_stop_preserve_every_actual_sample_once(self):
        live = self.create()
        source = pcm(FRAME * 5 + 137, 2234)
        cursor = 0
        for samples in [3, 509, 1, 511, 177, 384, 608, 1000]:
            block = source[cursor:cursor + samples * 2]
            live.feed(block)
            cursor += len(block)
        live.feed(source[cursor:])
        live.finish()
        live.finish()
        live.feed(pcm(FRAME, 9999))
        self.assertEqual(len(self.utterances()), 1)
        self.assertEqual(self.read(self.utterances()[0]), source)
        self.assertFalse(self.utterances()[0]["cancelled"])

    def test_terminal_frame_is_classified_without_an_active_phrase_and_wav_has_no_padding(self):
        detector = Detector()
        live = self.create(detector)
        silence, tail = pcm(FRAME * 25), pcm(123, 2345)
        live.feed(silence)
        live.feed(tail)
        self.assertIsNone(live.segmenter.id)
        live.finish()
        self.assertEqual(detector.calls, 26)
        self.assertTrue(np.all(detector.last[:123] != 0))
        self.assertTrue(np.all(detector.last[123:] == 0))
        self.assertEqual(len(self.utterances()), 1)
        self.assertEqual(self.read(self.utterances()[0]), silence[-9600 * 2:] + tail)
        live.finish()
        self.assertEqual(len(self.utterances()), 1)

    def test_cancel_keeps_exact_terminal_audio_but_never_announces_it_as_deliverable(self):
        live = self.create()
        source = pcm(FRAME * 4 + 79, 3456)
        live.feed(source)
        live.finish(cancelled=True)
        live.finish(cancelled=False)
        self.assertEqual(len(self.utterances()), 1)
        self.assertTrue(self.utterances()[0]["cancelled"])
        self.assertEqual(self.read(self.utterances()[0]), source)

    def test_ambiguous_frames_reset_possible_silence_without_splitting_a_phrase(self):
        live = self.create(Detector([0.9] * 4 + [0.4] * 19 + [0.0] * 19))
        live.feed(pcm(FRAME * 4, 1))
        live.feed(pcm(FRAME * 19, 2))
        self.assertEqual(self.utterances(), [])
        live.feed(pcm(FRAME * 19))
        self.assertEqual(len(self.utterances()), 1)

    def test_live_speech_is_not_forcibly_split_at_the_legacy_30_second_limit(self):
        live = self.create()
        source = pcm(FRAME * 950, 4567)
        live.feed(source)
        self.assertEqual(self.utterances(), [])
        self.assertEqual(live.segmenter.max_utterance, 300 * RATE)
        live.finish()
        self.assertEqual(len(self.utterances()), 1)
        self.assertEqual(self.read(self.utterances()[0]), source)

    def test_requested_live_limit_is_always_finite_and_capped_at_five_minutes(self):
        live = self.create(max_phrase_secs=99999)
        self.assertEqual(live.segmenter.max_utterance, RATE * 300)

    def test_limit_seals_before_session_error_and_cleanup_cannot_duplicate_the_phrase(self):
        request = {"op": "listen_start", "id": "session", "directory": self.temp.name,
                   "silence_ms": 600, "max_phrase_secs": 1}
        messages = [request,
                    {"op": "pcm", "id": "session", "data": base64.b64encode(pcm(FRAME * 32, 5678)).decode()},
                    {"op": "pcm", "id": "session", "data": base64.b64encode(pcm(FRAME, 6789)).decode()},
                    {"op": "listen_stop", "id": "session"}, {"op": "shutdown"}]
        live_type = worker.LiveVad
        detector = Detector()
        with patch.object(worker, "LiveVad", side_effect=lambda req: live_type(req, detector=detector)), \
             patch.object(worker, "Engine", return_value=SimpleNamespace()), \
             patch.dict(sys.modules, {"faster_whisper": SimpleNamespace()}), \
             patch.object(sys, "stdin", io.StringIO("\n".join(json.dumps(message) for message in messages))):
            worker.main()
        self.assertEqual(len(self.utterances()), 1)
        self.assertEqual(self.read(self.utterances()[0]), pcm(FRAME * 32, 5678))
        names = [event for event, _ in self.events]
        self.assertLess(names.index("utterance"), names.index("error"))
        errors = [values for event, values in self.events if event == "error"]
        self.assertEqual(len(errors), 1)
        self.assertEqual(errors[0]["id"], "session")
        self.assertIn("Limite", errors[0]["message"])
        self.assertEqual(detector.calls, 32)
        self.assertIn("listen_stopped", names)


class RecurrentAdapterTests(unittest.TestCase):
    class Session:
        def __init__(self):
            self.inputs = []

        def get_inputs(self):
            return [SimpleNamespace(name=name) for name in ("input", "h", "c")]

        def run(self, outputs, values):
            self.inputs.append({key: value.copy() for key, value in values.items()})
            return np.array([[0.8]], dtype=np.float32), values["h"] + 1, values["c"] + 2

    def test_state_and_64_sample_context_continue_but_new_listening_is_independent(self):
        session = self.Session()
        first = StatefulSilero(session)
        first.probability(np.arange(FRAME, dtype=np.float32))
        first.probability(np.zeros(FRAME, dtype=np.float32))
        self.assertTrue(np.all(session.inputs[1]["h"] == 1))
        self.assertTrue(np.all(session.inputs[1]["c"] == 2))
        np.testing.assert_array_equal(session.inputs[1]["input"][0, :64], np.arange(FRAME - 64, FRAME))
        second = StatefulSilero(session)
        second.probability(np.ones(FRAME, dtype=np.float32))
        self.assertTrue(np.all(session.inputs[2]["h"] == 0))
        self.assertTrue(np.all(session.inputs[2]["c"] == 0))
        self.assertTrue(np.all(session.inputs[2]["input"][0, :64] == 0))

    def test_incompatible_model_and_invalid_frames_are_rejected_before_inference(self):
        with self.assertRaises(RuntimeError):
            StatefulSilero(SimpleNamespace(get_inputs=lambda: [SimpleNamespace(name="state")]))
        session = self.Session()
        detector = StatefulSilero(session)
        with self.assertRaises(ValueError):
            detector.probability(np.zeros(FRAME - 1, dtype=np.float32))
        frame = np.zeros(FRAME, dtype=np.float32)
        frame[0] = float("nan")
        with self.assertRaises(ValueError):
            detector.probability(frame)
        self.assertEqual(session.inputs, [])


if __name__ == "__main__":
    unittest.main(verbosity=2)
