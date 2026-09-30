"""The test suite asserts the Korean messages; English is covered by test_i18n.py."""

from __future__ import annotations

import pytest

from tablegen.i18n import set_language


@pytest.fixture(autouse=True)
def _korean_messages():
    set_language("ko")
    yield
    set_language("ko")
