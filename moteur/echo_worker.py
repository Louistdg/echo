#!/usr/bin/env python3
"""Écho V3: offline faster-whisper worker, JSON lines over standard I/O.

The transcription thread keeps the preview and final models resident. The input thread also
runs Silero on a sliding audio window for continuous, explicitly armed listening.
No microphone is opened here, no network endpoint is created, no text is logged.
"""
from __future__ import annotations

import base64
import collections
from contextlib import contextmanager, nullcontext
import gc
import json
import math
import os
from pathlib import Path
import queue
import sys
import threading
import time
import uuid
import wave

os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")
os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
RATE = 16000
BLOCK = 3200  # 200 ms: bounded IPC and VAD work, with a 600 ms pre-roll.
MAX_UTTERANCE = RATE * 30
_PRINT = threading.Lock()


def emit(event: str, **values):
    with _PRINT:
        try:
            print(json.dumps({"event": event, **values}, ensure_ascii=False), flush=True)
        except (BrokenPipeError, OSError):
            os._exit(0)


class Segmenter:
    """Sample-counted utterance boundaries, independent of audio inference.

    Only the short pre-roll lives in memory during silence. Speech is bounded to
    30 seconds and sealed as PCM16 WAV before announcing it to the parent.
    """

    def __init__(self, directory: str, session: str, silence_ms: int, send=emit):
        self.directory = Path(directory)
        self.directory.mkdir(parents=True, exist_ok=True)
        self.session = session
        self.silence = RATE * max(350, min(6000, silence_ms)) // 1000
        self.send = send
        self.preroll = collections.deque(maxlen=3)
        self.audio = bytearray()
        self.id = None
        self.quiet = 0
        self.voiced_samples = 0

    def consume(self, pcm: bytes, voiced: bool):
        samples = len(pcm) // 2
        if self.id is None:
            if not voiced:
                self.preroll.append(pcm)
                return
            self.id = str(uuid.uuid4())
            self.audio = bytearray(b"".join(self.preroll))
            self.preroll.clear()
            self.quiet = self.voiced_samples = 0
            self.send("speech_start", session=self.session, id=self.id,
                      path=str(self.directory / (self.id + ".wav")))
        self.audio.extend(pcm)
        if voiced:
            self.voiced_samples += samples
            self.quiet = 0
        else:
            self.quiet += samples
        if self.quiet >= self.silence or len(self.audio) // 2 >= MAX_UTTERANCE:
            self.finish()

    def finish(self, cancelled=False):
        if self.id is None:
            return
        identity, audio, spoken = self.id, bytes(self.audio), self.voiced_samples
        self.id = None
        self.audio.clear()
        self.preroll.clear()
        self.quiet = self.voiced_samples = 0
        # A short syllable can still be useful; the final VAD filter decides
        # whether there is speech. Empty and sub-100ms triggers are ignored.
        if spoken < RATE // 10:
            self.send("speech_empty", session=self.session, id=identity)
            return
        final = self.directory / (identity + ".wav")
        temporary = self.directory / (identity + ".wav.part")
        with temporary.open("xb") as raw:
            with wave.open(raw, "wb") as wav:
                wav.setnchannels(1)
                wav.setsampwidth(2)
                wav.setframerate(RATE)
                wav.writeframes(audio)
            raw.flush()
            os.fsync(raw.fileno())
        # The UUID and exclusive temporary creation prevent overwrite.
        temporary.rename(final)
        self.send("utterance", session=self.session, id=identity,
                  path=str(final), duration_secs=len(audio) / (RATE * 2),
                  cancelled=cancelled)


class LiveVad:
    def __init__(self, request):
        import numpy as np
        from faster_whisper.vad import get_speech_timestamps, get_vad_model
        get_vad_model()  # Fail before announcing that hands-free is ready.
        self.np = np
        self.detect = get_speech_timestamps
        self.segmenter = Segmenter(request["directory"], request["id"],
                                   int(request.get("silence_ms", 900)))
        self.pending = bytearray()
        self.context = bytearray()

    def feed(self, pcm):
        if len(pcm) % 2:
            raise ValueError("PCM16 incomplet")
        self.pending.extend(pcm)
        while len(self.pending) >= BLOCK * 2:
            block = bytes(self.pending[:BLOCK * 2])
            del self.pending[:BLOCK * 2]
            self.context.extend(block)
            # 1.2 s gives Silero speech context while keeping each call bounded.
            if len(self.context) > RATE * 2 * 6 // 5:
                del self.context[:-RATE * 2 * 6 // 5]
            audio = self.np.frombuffer(bytes(self.context), dtype="<i2").astype("float32") / 32768
            speech = self.detect(audio, sampling_rate=RATE,
                                 min_speech_duration_ms=80,
                                 min_silence_duration_ms=100,
                                 speech_pad_ms=0, threshold=0.5)
            voiced = any(s["end"] > len(audio) - BLOCK for s in speech)
            self.segmenter.consume(block, voiced)

    def finish(self, cancelled=False):
        # Preserve the remaining (<200 ms) samples when stopping mid-phrase.
        if self.segmenter.id is not None and self.pending:
            self.segmenter.audio.extend(self.pending)
        self.pending.clear()
        self.segmenter.finish(cancelled=cancelled)


def read_audio(path):
    import numpy as np
    with wave.open(str(path), "rb") as wav:
        if wav.getsampwidth() != 2:
            raise ValueError("Le moteur attend un enregistrement PCM 16 bits.")
        channels, rate = wav.getnchannels(), wav.getframerate()
        if rate <= 0 or channels <= 0:
            raise ValueError("En-tête WAV invalide")
        if wav.getnframes() > rate * 3600:
            raise ValueError("Enregistrement supérieur à une heure")
        audio = np.frombuffer(wav.readframes(wav.getnframes()), dtype="<i2").astype("float32") / 32768
    if channels > 1:
        audio = audio.reshape(-1, channels).mean(axis=1)
    if rate != RATE and len(audio):
        size = int(round(len(audio) * RATE / rate))
        audio = np.interp(np.arange(size) * rate / RATE,
                          np.arange(len(audio)), audio).astype("float32")
    return audio



def normalize_hotwords(value):
    """Hints only: bounded vocabulary, never instructions or replacement rules."""
    if value is None:
        return None
    if isinstance(value, list):
        if not all(isinstance(word, str) for word in value):
            raise ValueError("Le lexique doit contenir des mots.")
        value = " ".join(value[:200])
    if not isinstance(value, str):
        raise ValueError("Le lexique doit être une chaîne ou une liste de mots.")
    value = " ".join(value.split())
    return value[:800] or None


# --- choix du materiel -------------------------------------------------------

# Mesure sur cette machine, 3,68 s d'audio, faster-whisper 1.2.1 :
#   int8 / 4 threads (reglage d'origine)  : 7,5 s
#   int8 / 8 threads                     : 4,8 s
#   int8_float32 / 8 threads             : 3,9 s
# int8_float32 garde les activations en float32 : plus rapide ET plus juste
# que int8, qui reste le choix par defaut de faster-whisper.
MEILLEUR_TYPE_CPU = "int8_float32"

_vad_chauffe = False


def candidats_materiel():
    """Rend la liste des (device, compute_type, threads) a essayer, GPU d'abord.

    Le GPU n'est pas adopte sur parole : CTranslate2 peut construire un modele
    CUDA puis echouer a l'inference (cuBLAS absent sur un Maxwell, par exemple).
    C'est la verification d'inference, plus bas, qui decide.
    """
    threads = max(2, os.cpu_count() or 4)
    candidats = []
    try:
        import ctranslate2

        if ctranslate2.get_cuda_device_count() > 0:
            supportes = ctranslate2.get_supported_compute_types("cuda")
            for type_calcul in ("float16", "int8_float16", "int8", "float32"):
                if type_calcul in supportes:
                    candidats.append(("cuda", type_calcul, threads))
                    break
    except Exception:
        pass
    candidats.append(("cpu", MEILLEUR_TYPE_CPU, threads))
    candidats.append(("cpu", "int8", min(4, max(2, threads - 2))))
    return candidats


def _verifier_inference(modele):
    """Force un vrai passage dans le modele : 0,5 s de silence.

    C'est ce qui fait apparaitre les erreurs de bibliotheque CUDA, qui ne se
    declarent qu'a l'inference. Sans cette verification, le worker adopte un
    modele GPU inutilisable et la dictee echoue (voire reste bloquee).
    """
    import numpy as np

    silence = np.zeros(8000, dtype=np.float32)
    list(modele.transcribe(silence, language="fr", beam_size=1,
                           without_timestamps=True)[0])


def charger_modele(name):
    """Charge le modele sur le premier materiel qui fonctionne VRAIMENT.

    Rend (modele, device, compute_type, threads). Leve si aucun ne marche.
    """
    from faster_whisper import WhisperModel

    derniere = None
    for device, compute_type, threads in candidats_materiel():
        try:
            modele = WhisperModel(name, device=device, compute_type=compute_type,
                                  cpu_threads=threads, local_files_only=True)
            _verifier_inference(modele)
            if device != "cpu":
                emit("engine", status="accelere", device=device,
                     compute_type=compute_type)
            return modele, device, compute_type, threads
        except Exception as exc:
            derniere = exc
            emit("warning",
                 message=f"{device}/{compute_type} inutilisable ({str(exc)[:140]})")
    raise derniere if derniere else RuntimeError("aucun materiel disponible")


def chauffer_vad():
    """Charge le modele Silero en tache de fond.

    get_vad_model() coute environ 15 s sur cette machine. Sans ce prechargement,
    l'utilisateur attend ce temps au moment precis ou il active le mains libres
    (LiveVad.__init__ appelle get_vad_model). Le faire ici, pendant que la
    premiere transcription travaille deja, retire cette attente du parcours.
    """
    global _vad_chauffe

    if _vad_chauffe:
        return
    _vad_chauffe = True

    def travail():
        try:
            from faster_whisper.vad import get_vad_model

            debut = time.monotonic()
            get_vad_model()
            emit("engine", status="vad-ready",
                 seconds=round(time.monotonic() - debut, 1))
        except Exception as exc:  # le VAD reste facultatif
            emit("warning", message=f"VAD non precharge : {exc}")

    threading.Thread(target=travail, daemon=True).start()


def transcription_options(request):
    preview = request.get("preview") is True
    # Final text keeps beam search in both profiles. Greedy Base decoding
    # degraded real dictation; reserve it for provisional previews.
    options = {
        "beam_size": 1 if preview else 5,
        "temperature": 0.0,
        "vad_filter": True,
        "condition_on_previous_text": False,
        "without_timestamps": not preview,
        "vad_parameters": {"min_silence_duration_ms": 350, "speech_pad_ms": 200},
    }
    if preview:
        options["word_timestamps"] = True
    hints = normalize_hotwords(request.get("hotwords"))
    if hints:
        options["hotwords"] = hints
    return options


class PreviewSuperseded(Exception):
    """An optional preview yielded the decoder to a durable final request."""


@contextmanager
def inference_priority(preview=False):
    """Give final dictation a short scheduling boost, then restore Windows.

    Keep the same model and decoding math. Preview jobs and explicitly chosen
    non-normal priorities are left alone. Unsupported/denied boosts are optional.
    """
    changed = False
    try:
        if os.name == "nt" and not preview:
            import ctypes
            from ctypes import wintypes
            kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel.GetCurrentProcess.restype = wintypes.HANDLE
            kernel.GetPriorityClass.argtypes = [wintypes.HANDLE]
            kernel.GetPriorityClass.restype = wintypes.DWORD
            kernel.SetPriorityClass.argtypes = [wintypes.HANDLE, wintypes.DWORD]
            kernel.SetPriorityClass.restype = wintypes.BOOL
            process = kernel.GetCurrentProcess()
            previous = kernel.GetPriorityClass(process)
            if previous == 0x20:  # NORMAL -> ABOVE_NORMAL, never high/realtime.
                changed = bool(kernel.SetPriorityClass(process, 0x8000))
    except (OSError, AttributeError):
        pass
    try:
        yield
    finally:
        if changed:
            kernel.SetPriorityClass(process, previous)


@contextmanager
def preview_encoder_context(duration, preview=False):
    """Shorten zero-padding only for provisional Base previews."""
    if not preview:
        yield
        return
    import faster_whisper as faster_whisper_package
    if not hasattr(faster_whisper_package, "__path__"):
        yield
        return
    import faster_whisper.transcribe as transcribe_module
    from faster_whisper.audio import pad_or_trim as vendor_pad_or_trim

    original = transcribe_module.pad_or_trim
    target_frames = max(800, min(3000, int(math.ceil((duration + 1.5) * 100))))

    def sized_pad(array, length=3000, *, axis=-1):
        return vendor_pad_or_trim(array, length=target_frames, axis=axis)

    transcribe_module.pad_or_trim = sized_pad
    try:
        yield
    finally:
        transcribe_module.pad_or_trim = original


@contextmanager
def preview_guard(model, cancel):
    # The vendor generator otherwise checks cancellation only after a complete
    # segment, including decoding and word alignment. A native encode call is
    # not interruptible; yield at its next safe boundary, without touching any
    # final transcription options or monkey-patching the vendor module.
    restored = []
    def check():
        if cancel.is_set():
            raise PreviewSuperseded()
    def guarded(original):
        def call(*args, **kwargs):
            check()
            result = original(*args, **kwargs)
            check()
            return result
        return call
    try:
        check()
        for name in ("encode", "generate_with_fallback", "add_word_timestamps"):
            original = getattr(model, name, None)
            if original is None:
                continue
            own = name in vars(model)
            restored.append((name, own, original))
            setattr(model, name, guarded(original))
        yield
    finally:
        for name, own, original in reversed(restored):
            if own:
                setattr(model, name, original)
            else:
                delattr(model, name)


class Engine:
    def __init__(self):
        self.jobs = queue.Queue(maxsize=4)
        self.events = {}
        self.events_lock = threading.Lock()
        self.preview_pending = None
        self.preview_active = None
        self.wake = threading.Event()
        self.last_work = time.monotonic()
        self.model = None
        self.model_name = None
        self.models = collections.OrderedDict()
        threading.Thread(target=self.run, name="echo-transcription", daemon=True).start()

    def submit(self, request):
        cancel = threading.Event()
        superseded = None
        with self.events_lock:
            self.events[request["id"]] = cancel
            if request.get("preview") is True:
                superseded = self.preview_pending
                self.preview_pending = (request, cancel)
                if superseded:
                    old, old_cancel = superseded
                    old_cancel.set()
                    if self.events.get(old["id"]) is old_cancel:
                        self.events.pop(old["id"], None)
        if request.get("preview") is True:
            if superseded:
                emit("cancelled", id=superseded[0]["id"])
            self.wake.set()
            return
        try:
            self.jobs.put_nowait((request, cancel))
        except queue.Full:
            with self.events_lock:
                if self.events.get(request["id"]) is cancel:
                    self.events.pop(request["id"], None)
            emit("error", id=request["id"], message="La file de transcription est pleine.")
        else:
            if request.get("op") != "preload":
                with self.events_lock:
                    if self.preview_active:
                        self.preview_active[1].set()
                    pending, self.preview_pending = self.preview_pending, None
                    if pending:
                        old, old_cancel = pending
                        old_cancel.set()
                        if self.events.get(old["id"]) is old_cancel:
                            self.events.pop(old["id"], None)
                if pending:
                    emit("cancelled", id=old["id"])
        self.wake.set()

    def idle_preload(self):
        # Select the cached preview model between dictations. The final model
        # stays resident, so the next phrase does not reload it from disk.
        if self.model_name in (None, "base") or time.monotonic() - self.last_work < 5:
            return False
        if not self.jobs.empty():
            return False
        with self.events_lock:
            if self.preview_pending is not None:
                return False
        try:
            self.load("base")
        except Exception:
            # Idle preparation is optional. A missing cache must not kill the
            # scheduler or prevent a later explicit final request/retry.
            emit("engine", status="cold", model="base")
            self.last_work = time.monotonic()
            return False
        self.last_work = time.monotonic()
        return True

    def next_job(self):
        # Durable final requests take priority. At most one preview waits;
        # newer snapshots replace it rather than delaying Stop behind a FIFO.
        while True:
            try:
                request, cancel = self.jobs.get_nowait()
                return request, cancel, True
            except queue.Empty:
                pass
            with self.events_lock:
                # A final can arrive between the queue check and this lock.
                if not self.jobs.empty():
                    continue
                pending, self.preview_pending = self.preview_pending, None
                if pending:
                    self.preview_active = (pending[0]["id"], pending[1])
            if pending:
                return pending[0], pending[1], False
            self.idle_preload()
            self.wake.wait(0.1)
            self.wake.clear()

    def cancel(self, identity):
        with self.events_lock:
            event = self.events.get(identity)
        if event:
            event.set()

    def load(self, name):
        if name in self.models:
            self.model = self.models[name]
            self.model_name = name
            self.models.move_to_end(name)
            return
        # Bound memory to two instances, including when a user changes model.
        # A failed construction leaves any other cached model usable.
        if len(self.models) >= 2:
            old_name = next((key for key in self.models if key != "base"), next(iter(self.models)))
            previous = self.models.pop(old_name)
            if previous is self.model:
                self.model = None
                self.model_name = None
            if hasattr(previous, "model") and hasattr(previous.model, "unload_model"):
                previous.model.unload_model(to_cpu=False)
            del previous
            gc.collect()
        emit("engine", status="loading", model=name)

        self.model, device, compute_type, threads = charger_modele(name)
        self.model_name = name
        self.models[name] = self.model
        emit("engine", status="ready", model=name, device=device,
             compute_type=compute_type, cpu_threads=threads)
        chauffer_vad()

    def run(self):
        while True:
            request, cancel, durable = self.next_job()
            identity = request["id"]
            try:
                if cancel.is_set():
                    emit("cancelled", id=identity)
                    continue
                if request.get("op") == "preload":
                    self.load("base")
                    self.load(request.get("model") or "small")
                    emit("preloaded", id=identity)
                    continue
                self.load("base" if request.get("preview") is True else (request.get("model") or "small"))
                if cancel.is_set():
                    emit("cancelled", id=identity)
                    continue
                audio = read_audio(request["path"])
                duration = len(audio) / RATE
                if duration < 0.1:
                    emit("result", id=identity, text="", duration_secs=duration)
                    continue
                emit("started", id=identity, duration_secs=duration)
                language = request.get("language") or "fr"
                if language == "auto":
                    language = None
                preview = request.get("preview") is True
                guard = preview_guard(self.model, cancel) if preview else nullcontext()
                preview_context = preview_encoder_context(duration, preview=preview)
                with inference_priority(preview=preview), guard, preview_context:
                    segments, _ = self.model.transcribe(
                        audio, language=language, **transcription_options(request))
                    texts = []
                    words = []
                    for segment in segments:
                        if cancel.is_set():
                            break
                        value = segment.text.strip()
                        if value:
                            texts.append(value)
                        if request.get("preview") is True:
                            words.extend({"word": word.word, "start": word.start, "end": word.end}
                                         for word in (getattr(segment, "words", None) or []))
                        emit("progress", id=identity,
                             progress=min(0.99, max(0, segment.end / duration)),
                             text=" ".join(texts),
                             **({"words": words} if request.get("preview") is True else {}))
                if cancel.is_set():
                    emit("cancelled", id=identity)
                else:
                    emit("result", id=identity, text=" ".join(texts).strip(),
                         duration_secs=duration,
                         **({"words": words} if request.get("preview") is True else {}))
            except PreviewSuperseded:
                emit("cancelled", id=identity)
            except Exception as exc:
                # No audio or transcript is written to logs.
                emit("error", id=identity, message=f"{type(exc).__name__}: {exc}")
            finally:
                with self.events_lock:
                    if self.preview_active and self.preview_active[0] == identity:
                        self.preview_active = None
                    if self.events.get(identity) is cancel:
                        self.events.pop(identity, None)
                self.last_work = time.monotonic()
                if durable:
                    self.jobs.task_done()


def main():
    # Force UTF-8 on Windows regardless of console code page.
    for stream in (sys.stdin, sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    emit("hello", protocol=1)
    # Initialise native extension modules on the main thread. On Windows,
    # NumPy's first import can block when performed by the decoding thread.
    # The model itself is still constructed and kept resident by Engine.
    initialization_error = None
    try:
        import numpy
        import faster_whisper
    except Exception as exc:
        initialization_error = f"{type(exc).__name__}: {exc}"
    engine = Engine()
    listening = None
    for raw in sys.stdin:
        request = {}
        try:
            request = json.loads(raw)
            op = request.get("op")
            if initialization_error and op in ("preload", "transcribe", "listen_start"):
                emit("error", id=request.get("id"), message=initialization_error)
                continue
            if op in ("preload", "transcribe"):
                engine.submit(request)
            elif op == "cancel":
                engine.cancel(request["id"])
            elif op == "listen_start":
                if listening:
                    listening.finish(cancelled=True)
                listening = LiveVad(request)
                emit("listening", id=request["id"], vad="silero")
            elif op == "pcm":
                if listening and request.get("id") == listening.segmenter.session:
                    listening.feed(base64.b64decode(request["data"], validate=True))
            elif op == "listen_stop":
                if listening and request.get("id") == listening.segmenter.session:
                    listening.finish(cancelled=bool(request.get("cancelled")))
                    listening = None
                emit("listen_stopped", id=request.get("id"))
            elif op == "shutdown":
                if listening:
                    listening.finish(cancelled=True)
                break
            elif op == "ping":
                emit("pong", id=request.get("id"))
            else:
                raise ValueError("Commande de moteur inconnue")
        except Exception as exc:
            emit("error", id=request.get("id"),
                 message=f"{type(exc).__name__}: {exc}")
            if request.get("op") in ("pcm", "listen_start"):
                if listening:
                    try:
                        listening.finish(cancelled=True)
                    except Exception:
                        pass
                listening = None


def self_test():
    import tempfile
    import unittest

    class StartupTests(unittest.TestCase):
        def run_main(self, fail_import=False):
            import builtins
            import io
            from types import SimpleNamespace
            from unittest.mock import patch
            calls, events = [], []
            original_import = builtins.__import__
            def fake_import(name, *args, **kwargs):
                if name in ("numpy", "faster_whisper"):
                    calls.append((name, threading.current_thread()))
                    if fail_import:
                        raise ImportError("runtime indisponible")
                    return SimpleNamespace()
                return original_import(name, *args, **kwargs)
            def fake_engine():
                calls.append(("Engine", threading.current_thread()))
                return SimpleNamespace(submit=lambda request: calls.append(("submit", request)))
            messages = '{"op":"preload","id":"startup-test"}\n{"op":"shutdown"}\n'
            with patch.object(builtins, "__import__", side_effect=fake_import), \
                 patch.object(sys, "stdin", io.StringIO(messages)), \
                 patch.object(sys.modules[__name__], "Engine", side_effect=fake_engine), \
                 patch.object(sys.modules[__name__], "emit", side_effect=lambda event, **values: events.append((event, values))):
                main()
            return calls, events

        def test_native_imports_run_on_main_thread_before_engine(self):
            calls, events = self.run_main()
            self.assertEqual([name for name, _ in calls], ["numpy", "faster_whisper", "Engine", "submit"])
            self.assertTrue(all(thread is threading.main_thread() for _, thread in calls[:3]))
            self.assertEqual(events[0], ("hello", {"protocol": 1}))

        def test_initialization_failure_keeps_request_error_protocol(self):
            calls, events = self.run_main(fail_import=True)
            self.assertFalse(any(name == "submit" for name, _ in calls))
            self.assertEqual(events, [("hello", {"protocol": 1}),
                                     ("error", {"id": "startup-test", "message": "ImportError: runtime indisponible"})])

    class SegmentTests(unittest.TestCase):
        def setUp(self):
            self.temp = tempfile.TemporaryDirectory(prefix="echo-vad-test-")
            self.events = []
            self.vad = Segmenter(self.temp.name, "test", 600,
                                 send=lambda event, **args: self.events.append((event, args)))
            self.block = b"\x01\x00" * BLOCK

        def tearDown(self):
            self.temp.cleanup()

        def test_silence_never_writes_audio(self):
            for _ in range(100):
                self.vad.consume(self.block, False)
            self.assertEqual(list(Path(self.temp.name).iterdir()), [])
            self.assertEqual(len(self.vad.preroll), 3)

        def test_speech_has_preroll_and_is_sealed_after_pause(self):
            for _ in range(5):
                self.vad.consume(self.block, False)
            for _ in range(4):
                self.vad.consume(self.block, True)
            for _ in range(3):
                self.vad.consume(self.block, False)
            self.assertEqual([e[0] for e in self.events], ["speech_start", "utterance"])
            final = Path(self.events[-1][1]["path"])
            with wave.open(str(final), "rb") as wav:
                self.assertEqual(wav.getnframes(), 10 * BLOCK)
                self.assertEqual((wav.getframerate(), wav.getnchannels()), (16000, 1))
            self.assertFalse(list(final.parent.glob("*.part")))

        def test_continuous_speech_is_bounded_and_names_unique(self):
            for _ in range(151):
                self.vad.consume(self.block, True)
            self.vad.finish()
            paths = [e[1]["path"] for e in self.events if e[0] == "utterance"]
            self.assertEqual(len(paths), 2)
            self.assertNotEqual(paths[0], paths[1])
            with wave.open(paths[0], "rb") as wav:
                self.assertLessEqual(wav.getnframes(), MAX_UTTERANCE)

        def test_cancel_retains_audio_but_marks_it_cancelled(self):
            self.vad.consume(self.block, True)
            self.vad.finish(cancelled=True)
            event = self.events[-1]
            self.assertTrue(event[1]["cancelled"])
            self.assertTrue(Path(event[1]["path"]).is_file())

    class EngineTests(unittest.TestCase):
        @unittest.skipUnless(os.name == "nt", "Windows scheduling only")
        def test_final_priority_restored_after_failure_and_preview_unchanged(self):
            import ctypes
            from ctypes import wintypes
            kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel.GetCurrentProcess.restype = wintypes.HANDLE
            kernel.GetPriorityClass.argtypes = [wintypes.HANDLE]
            kernel.GetPriorityClass.restype = wintypes.DWORD
            process = kernel.GetCurrentProcess()
            previous = kernel.GetPriorityClass(process)
            with inference_priority(preview=True):
                self.assertEqual(kernel.GetPriorityClass(process), previous)
            with self.assertRaisesRegex(RuntimeError, "decode failed"):
                with inference_priority():
                    observed = kernel.GetPriorityClass(process)
                    self.assertIn(observed, (previous, 0x8000) if previous == 0x20 else (previous,))
                    raise RuntimeError("decode failed")
            self.assertEqual(kernel.GetPriorityClass(process), previous)

        def test_preview_guard_restores_methods_after_failure(self):
            class FakeModel:
                def encode(self):
                    raise ValueError("encode failed")
            model = FakeModel()
            with self.assertRaises(ValueError):
                with preview_guard(model, threading.Event()):
                    model.encode()
            self.assertEqual(vars(model), {})
            with self.assertRaises(ValueError):
                model.encode()

        def test_final_decode_settings_still_keep_precision(self):
            quick = transcription_options({"model": "base"})
            precise = transcription_options({"model": "small"})
            preview = transcription_options({"model": "small", "preview": True})
            self.assertEqual(quick["beam_size"], 5)
            self.assertEqual(precise["beam_size"], 5)
            self.assertEqual(preview["beam_size"], 1)
            options = precise
            self.assertTrue(options["vad_filter"])
            self.assertEqual(options["temperature"], 0.0)
            self.assertTrue(options["without_timestamps"])
            self.assertNotIn("chunk_length", options)

        def test_running_preview_yields_before_generation_and_final_succeeds(self):
            from types import SimpleNamespace
            from unittest.mock import patch
            entered, release, final_done = threading.Event(), threading.Event(), threading.Event()
            calls, events = [], []
            class FakeModel:
                def __init__(self, name, **kwargs):
                    self.name = name
                def encode(self):
                    if self.name == "base":
                        entered.set()
                        release.wait(2)
                def generate_with_fallback(self):
                    calls.append(self.name)
                def transcribe(self, audio, **kwargs):
                    def segments():
                        self.encode()
                        self.generate_with_fallback()
                        yield SimpleNamespace(text="La note précise.", end=1.0)
                    return segments(), None
            def emit_test(event, **values):
                events.append((event, values))
                if event == "result" and values["id"] == "final":
                    final_done.set()
            with patch.dict(sys.modules, {"faster_whisper": SimpleNamespace(WhisperModel=FakeModel)}), \
                 patch.object(sys.modules[__name__], "read_audio", return_value=[0.] * 32000), \
                 patch.object(sys.modules[__name__], "emit", side_effect=emit_test):
                engine = Engine()
                engine.submit({"op": "transcribe", "id": "preview", "path": "a.wav", "preview": True})
                self.assertTrue(entered.wait(2))
                engine.submit({"op": "transcribe", "id": "final", "path": "a.wav"})
                release.set()
                self.assertTrue(final_done.wait(3))
                engine.jobs.join()
            self.assertEqual(calls, ["small"])
            self.assertTrue(any(e == "cancelled" and v["id"] == "preview" for e, v in events))
            self.assertFalse(any(e == "error" for e, v in events))
            self.assertTrue(any(e == "result" and v["text"] == "La note précise." for e, v in events))

        def test_preview_is_stopped_between_encoder_and_decoder(self):
            from unittest.mock import patch
            cancel = threading.Event()
            calls = []
            class FakeModel:
                def encode(self):
                    calls.append("encode")
                    cancel.set()
                def generate_with_fallback(self):
                    calls.append("decode")
            model = FakeModel()
            with self.assertRaises(PreviewSuperseded):
                with preview_guard(model, cancel):
                    model.encode()
                    model.generate_with_fallback()
            self.assertEqual(calls, ["encode"])
            self.assertNotIn("encode", vars(model))
            model.generate_with_fallback()
            self.assertEqual(calls, ["encode", "decode"])

        def test_final_request_supersedes_preview_without_cancelling_other_finals(self):
            from unittest.mock import patch
            with patch.object(threading.Thread, "start"), patch.object(sys.modules[__name__], "emit"):
                engine = Engine()
                engine.submit({"id": "p", "preview": True})
                preview = engine.events["p"]
                engine.submit({"id": "f1", "op": "transcribe"})
                first = engine.events["f1"]
                self.assertTrue(preview.is_set())
                engine.submit({"id": "f2", "op": "transcribe"})
                self.assertFalse(first.is_set())
                self.assertFalse(engine.events["f2"].is_set())

        def test_failed_model_change_keeps_cached_preview_usable(self):
            from types import SimpleNamespace
            from unittest.mock import patch
            class FakeModel:
                def __init__(self, name, **kwargs):
                    if name == "missing":
                        raise RuntimeError("model absent")
                    self.model = SimpleNamespace(unload_model=lambda **kwargs: None)
            with patch.dict(sys.modules, {"faster_whisper": SimpleNamespace(WhisperModel=FakeModel)}), \
                 patch.object(sys.modules[__name__], "emit"):
                engine = Engine.__new__(Engine)
                engine.model = None;engine.model_name = None;engine.models = collections.OrderedDict()
                engine.load("base");engine.load("small")
                with self.assertRaises(RuntimeError):
                    engine.load("missing")
                self.assertEqual(set(engine.models), {"base"})
                engine.load("base")
                self.assertIs(engine.model, engine.models["base"])

        def test_preview_and_final_models_are_reused_without_reloading(self):
            from types import SimpleNamespace
            from unittest.mock import patch
            constructions, unloaded = [], []
            class FakeModel:
                def __init__(self, name, **kwargs):
                    constructions.append(name)
                    self.model = SimpleNamespace(unload_model=lambda **kwargs: unloaded.append(name))
            with patch.dict(sys.modules, {"faster_whisper": SimpleNamespace(WhisperModel=FakeModel)}), \
                 patch.object(sys.modules[__name__], "emit"):
                engine = Engine.__new__(Engine)
                engine.model = None;engine.model_name = None;engine.models = collections.OrderedDict()
                for name in ["base", "small", "base", "small", "base", "small"]:
                    engine.load(name)
                self.assertEqual(constructions, ["base", "small"])
                self.assertEqual(unloaded, [])
                self.assertEqual(set(engine.models), {"base", "small"})
                engine.load("medium")
                self.assertEqual(set(engine.models), {"base", "medium"})
                self.assertEqual(unloaded, ["small"])

        def test_model_stays_resident_and_jobs_keep_their_identity(self):
            from types import SimpleNamespace
            from unittest.mock import patch
            constructions, events = [], []
            class FakeModel:
                def __init__(self, name, **kwargs):
                    constructions.append(name)
                def transcribe(self, audio, **kwargs):
                    return iter([SimpleNamespace(text="Été.", end=1.0),
                                 SimpleNamespace(text="Œuf.", end=2.0)]), None
            with patch.dict(sys.modules, {"faster_whisper": SimpleNamespace(WhisperModel=FakeModel)}), \
                 patch.object(sys.modules[__name__], "read_audio", return_value=[0.] * 32000), \
                 patch.object(sys.modules[__name__], "emit", side_effect=lambda event, **values: events.append((event, values))):
                engine = Engine()
                engine.submit({"op": "transcribe", "id": "one", "path": "one.wav", "model": "small"})
                engine.submit({"op": "transcribe", "id": "two", "path": "two.wav", "model": "small"})
                engine.jobs.join()
            self.assertEqual(constructions, ["small"])
            results = [value for event, value in events if event == "result"]
            self.assertEqual([value["id"] for value in results], ["one", "two"])
            self.assertEqual(results[0]["text"], "Été. Œuf.")
            self.assertTrue(all(value["progress"] < 1 for event, value in events if event == "progress"))

        def test_cancel_during_decode_never_emits_a_result(self):
            from types import SimpleNamespace
            from unittest.mock import patch
            entered, release = threading.Event(), threading.Event()
            events = []
            class FakeModel:
                def __init__(self, *args, **kwargs):
                    pass
                def transcribe(self, audio, **kwargs):
                    def segments():
                        entered.set()
                        release.wait(2)
                        yield SimpleNamespace(text="Ce texte doit être annulé.", end=1.0)
                    return segments(), None
            with patch.dict(sys.modules, {"faster_whisper": SimpleNamespace(WhisperModel=FakeModel)}), \
                 patch.object(sys.modules[__name__], "read_audio", return_value=[0.] * 32000), \
                 patch.object(sys.modules[__name__], "emit", side_effect=lambda event, **values: events.append((event, values))):
                engine = Engine()
                engine.submit({"op": "transcribe", "id": "cancel-me", "path": "audio.wav"})
                self.assertTrue(entered.wait(2), "Le décodeur n'a pas démarré")
                engine.cancel("cancel-me")
                release.set()
                engine.jobs.join()
            self.assertTrue(any(event == "cancelled" for event, value in events))
            self.assertFalse(any(event == "result" for event, value in events))

    suite = unittest.TestSuite([
        unittest.defaultTestLoader.loadTestsFromTestCase(StartupTests),
        unittest.defaultTestLoader.loadTestsFromTestCase(SegmentTests),
        unittest.defaultTestLoader.loadTestsFromTestCase(EngineTests),
    ])
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    raise SystemExit(0 if result.wasSuccessful() else 1)


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        self_test()
    elif "--probe" in sys.argv:
        import faster_whisper
        import numpy
        from faster_whisper.vad import get_vad_model
        get_vad_model()
        print(json.dumps({"python": sys.executable, "faster_whisper": faster_whisper.__version__,
                          "numpy": numpy.__version__, "silero": "ready"}))
    else:
        main()
