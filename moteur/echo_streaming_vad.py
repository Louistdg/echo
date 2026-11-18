"""Per-listening recurrent state for the already installed Silero v6 session.

No Whisper model is constructed here. Phrase boundaries deliberately retain
the recurrent state; a new microphone session creates a fresh adapter instead.
"""
from __future__ import annotations

import math

RATE = 16000
FRAME = 512


class StatefulSilero:
    def __init__(self, session=None):
        import numpy as np
        if session is None:
            from faster_whisper.vad import get_vad_model
            session = get_vad_model().session
        if {item.name for item in session.get_inputs()} != {"input", "h", "c"}:
            raise RuntimeError("Le modèle local ne correspond pas à Silero v6 (input/h/c).")
        self.np = np
        self.session = session
        self.h = np.zeros((1, 1, 128), dtype=np.float32)
        self.c = np.zeros((1, 1, 128), dtype=np.float32)
        self.context = np.zeros((1, 64), dtype=np.float32)

    def probability(self, frame):
        np = self.np
        if frame.shape != (FRAME,) or not np.all(np.isfinite(frame)):
            raise ValueError("Une trame finie de 512 échantillons est requise.")
        data = np.concatenate((self.context, frame.reshape(1, FRAME)), axis=1)
        probability, h, c = self.session.run(None, {"input": data, "h": self.h, "c": self.c})
        value = float(np.asarray(probability).reshape(-1)[0])
        if not math.isfinite(value) or not 0 <= value <= 1:
            raise ValueError("Probabilité de parole invalide.")
        self.h, self.c = h, c
        self.context = frame[-64:].reshape(1, 64).copy()
        return value
