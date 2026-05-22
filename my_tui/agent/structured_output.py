from __future__ import annotations

import re
from typing import Any

_SECTION_PATTERN = re.compile(r"^###\s+(\w+)", re.MULTILINE)

_FIELDS = ("SUMMARY", "CHANGES", "EVIDENCE", "RISKS", "BLOCKERS")
_DEFAULT = "(未提供)"


def parse_structured_output(text: str) -> dict[str, str]:
    result: dict[str, str] = {field.lower(): _DEFAULT for field in _FIELDS}

    matches = list(_SECTION_PATTERN.finditer(text))
    for i, match in enumerate(matches):
        field = match.group(1).upper()
        if field not in _FIELDS:
            continue
        start = match.end()
        end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        value = text[start:end].strip()
        result[field.lower()] = value if value else _DEFAULT

    return result