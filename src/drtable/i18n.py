"""Message language for user-facing output (errors, warnings, reports).

Generated artifacts (C++, JSON, Markdown) are always in English so that they do not
change with the language setting. Only messages printed to the user are translated.

The language is chosen by ``--lang`` or the ``DRTABLE_LANG`` environment variable
(``en`` or ``ko``). English is the default.
"""

from __future__ import annotations

import os

SUPPORTED = ("en", "ko")
_language = os.environ.get("DRTABLE_LANG", "en").lower()
if _language not in SUPPORTED:
    _language = "en"


def set_language(language: str) -> None:
    global _language
    if language not in SUPPORTED:
        raise ValueError(f"unsupported language: {language}")
    _language = language


def language() -> str:
    return _language


def tr(ko: str, en: str) -> str:
    """Returns the message in the active language. Both texts are already formatted."""
    return ko if _language == "ko" else en
