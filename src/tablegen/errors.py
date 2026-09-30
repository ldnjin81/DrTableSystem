"""Validation error collection."""

from __future__ import annotations


class ValidationErrors(Exception):
    """Carries every validation error found in one run."""

    def __init__(self, messages: list[str]) -> None:
        self.messages = messages
        super().__init__("\n".join(messages))


class ErrorCollector:
    """Collects as many errors as possible before failing, each prefixed with Sheet!Cell."""

    def __init__(self) -> None:
        self.messages: list[str] = []

    def add(self, sheet: str, cell: str, message: str) -> None:
        self.messages.append(f"{sheet}!{cell}: {message}")

    def extend(self, messages: list[str]) -> None:
        self.messages.extend(messages)

    def raise_if_any(self) -> None:
        if self.messages:
            raise ValidationErrors(self.messages)
