"""검증 오류 수집 기능."""

from __future__ import annotations


class ValidationErrors(Exception):
    """여러 검증 오류를 한 번에 전달한다."""

    def __init__(self, messages: list[str]) -> None:
        self.messages = messages
        super().__init__("\n".join(messages))


class ErrorCollector:
    """가능한 오류를 모두 모은다."""

    def __init__(self) -> None:
        self.messages: list[str] = []

    def add(self, sheet: str, cell: str, message: str) -> None:
        self.messages.append(f"{sheet}!{cell}: {message}")

    def extend(self, messages: list[str]) -> None:
        self.messages.extend(messages)

    def raise_if_any(self) -> None:
        if self.messages:
            raise ValidationErrors(self.messages)
