#!/usr/bin/env python3
"""Offline French suggestions. One stdin JSON request, one stdout JSON response.
Text is never rewritten or persisted. Offsets and lengths are UTF-16 code units.
Bundled Grammalecte 2.3.0 is loaded locally, without pip or a background server.
"""
from __future__ import annotations
import contextlib
import hashlib
import io
import json
from pathlib import Path
import re
import sys
import unicodedata

ENGINE = "Grammalecte 2.3.0"
MAX_TEXT = 20000
MAX_INPUT = 256 * 1024
MAX_MATCHES = 250
_VENDOR = Path(__file__).resolve().parent / "vendor" / "grammalecte-2.3.0"
_CHECKER = None

def personal_words(value):
    if value is None:
        return set()
    if not isinstance(value, list) or len(value) > 200:
        raise ValueError("Le dictionnaire doit contenir au plus 200 mots.")
    result = set()
    for word in value:
        if not isinstance(word, str) or len(word) > 80 or "\n" in word or "\r" in word:
            raise ValueError("Chaque mot du dictionnaire doit contenir au plus 80 caractères.")
        word = unicodedata.normalize("NFC", word.strip()).casefold()
        if word:
            result.add(word)
    return result

def utf16_positions(text):
    positions = [0]
    for char in text:
        positions.append(positions[-1] + (2 if ord(char) > 0xffff else 1))
    return positions

def checker():
    global _CHECKER
    if _CHECKER is None:
        if not (_VENDOR / "grammalecte" / "grammar_checker.py").is_file():
            raise RuntimeError("Le correcteur français embarqué est absent.")
        sys.path.insert(0, str(_VENDOR))
        from grammalecte.grammar_checker import GrammarChecker
        _CHECKER = GrammarChecker("fr")
    return _CHECKER

def check(request):
    source = request.get("text", "") if isinstance(request, dict) else ""
    identity = request.get("request_id") if isinstance(request, dict) else None
    result = {"ok": False, "request_id": identity, "source": source if isinstance(source, str) else "",
              "matches": [], "engine": ENGINE}
    try:
        if not isinstance(request, dict) or not isinstance(source, str):
            raise ValueError("Le texte à vérifier doit être une chaîne.")
        if len(source) > MAX_TEXT:
            raise ValueError("Vérifie au plus 20 000 caractères à la fois.")
        # Reject lone surrogates explicitly rather than returning offsets which
        # cannot be safely represented by the Rust/UTF-8 transport.
        source.encode("utf-8", "strict")
        words = personal_words(request.get("personal_words"))
        if not source.strip():
            result["ok"] = True
            return result
        positions = utf16_positions(source)
        matches = []
        seen = set()
        # Keep the vendor's diagnostics out of the JSON transport and do not log
        # text. Original paragraphs/offsets are preserved, including CRLF.
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            gc = checker()
            for paragraph in re.finditer(r"[^\n]+", source):
                paragraph_text = paragraph.group(0)
                grammar, spelling = gc.getParagraphErrors(paragraph_text, bSpellSugg=False)
                for kind, errors in (("grammar", grammar), ("spelling", spelling)):
                    for error in errors:
                        local_start, local_end = int(error["nStart"]), int(error["nEnd"])
                        start, end = paragraph.start() + local_start, paragraph.start() + local_end
                        if not (0 <= local_start < local_end <= len(paragraph_text)):
                            continue
                        original = source[start:end]
                        folded = unicodedata.normalize("NFC", original).casefold()
                        if kind == "spelling" and folded in words:
                            continue
                        rule = str(error.get("sRuleId") or "unknown-word")
                        key = (start, end, rule, kind)
                        if key in seen:
                            continue
                        seen.add(key)
                        suggestions = error.get("aSuggestions") or []
                        if kind == "spelling":
                            suggestions = []
                            # Bound expensive graph searches for pathological
                            # unknown tokens and cap results sent to the UI.
                            if len(original) <= 64:
                                for group in gc.getSpellChecker().suggest(original, nSuggLimit=5):
                                    suggestions.extend(group)
                                    if len(suggestions) >= 5:
                                        break
                        suggestions = list(dict.fromkeys(s for s in suggestions
                                                         if isinstance(s, str) and s != original))[:5]
                        digest = hashlib.sha256((str(start) + ":" + str(end) + ":" + rule + ":" + kind).encode()).hexdigest()[:16]
                        matches.append({"id": digest, "offset": positions[start],
                                        "length": positions[end] - positions[start], "original": original,
                                        "message": str(error.get("sMessage") or "Mot absent du dictionnaire français."),
                                        "suggestions": suggestions, "kind": kind, "rule_id": rule})
                        if len(matches) >= MAX_MATCHES:
                            break
                    if len(matches) >= MAX_MATCHES:
                        break
                if len(matches) >= MAX_MATCHES:
                    break
        matches.sort(key=lambda match: (match["offset"], match["length"], match["kind"]))
        result.update(ok=True, matches=matches)
    except Exception as error:
        result["error"] = str(error) or type(error).__name__
    return result

def main():
    for stream in (sys.stdin, sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="strict")
    try:
        raw = sys.stdin.buffer.read(MAX_INPUT + 1)
        if len(raw) > MAX_INPUT:
            raise ValueError("La demande de correction est trop volumineuse.")
        request = json.loads(raw.decode("utf-8-sig"))
        result = check(request)
    except Exception as error:
        result = {"ok": False, "request_id": None, "source": "", "matches": [],
                  "engine": ENGINE, "error": str(error) or type(error).__name__}
    # Valid JSON error DTOs use exit 0 too: callers must inspect 'ok'.
    print(json.dumps(result, ensure_ascii=False), flush=True)
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
