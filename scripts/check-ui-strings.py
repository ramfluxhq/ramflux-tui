#!/usr/bin/env python3
"""Fail when production sources contain a hard-coded user-facing string.

Scans ``src/**/*.rs`` except the catalogs (``src/i18n/``) and the integration
tests (``src/tests.rs``). Skips comments, ``#[cfg(test)]`` items, and lines
marked ``ui-allow:``. A string is user-facing when it contains a letter and
also whitespace, CJK, or ``=``, or when it is a PascalCase interface label.
Identifier-like tokens and protocol component kinds are not interface copy.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
SKIP_FILES = {SRC / "tests.rs"}
PROTOCOL_KINDS = {
    "Text",
    "Divider",
    "Row",
    "Column",
    "List",
    "Card",
    "Button",
    "TextField",
    "DateTimeInput",
    "Slider",
    "CheckBox",
    "ChoicePicker",
}
IDENT = re.compile(r"^[a-z0-9_./:-]+$")
PASCAL = re.compile(r"^[A-Z][a-z]+(?:[A-Z][a-z0-9]+)*$")
LETTER = re.compile(r"[A-Za-z]")
CJK = re.compile(r"[\u3400-\u9fff]")


def main() -> int:
    findings: list[str] = []
    for path in sorted(SRC.rglob("*.rs")):
        if path in SKIP_FILES or "i18n" in path.parts:
            continue
        findings.extend(scan(path))
    if findings:
        print("\n".join(findings))
        return 1
    return 0


def scan(path: pathlib.Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    findings: list[str] = []
    brace = 0
    paren = 0
    skip_base: int | None = None
    seen_brace = False
    pending_cfg = False
    index = 0
    line = 1
    while index < len(text):
        if text.startswith("\n", index):
            line += 1
            index += 1
            continue
        if text.startswith("//", index):
            index = text.find("\n", index)
            if index < 0:
                break
            continue
        if text.startswith("/*", index):
            end = text.find("*/", index + 2)
            if end < 0:
                break
            line += text[index:end].count("\n")
            index = end + 2
            continue
        if text.startswith("#[", index):
            end = matching_bracket(text, index + 1)
            if end < 0:
                break
            attr = text[index : end + 1]
            line += attr.count("\n")
            if attr.replace(" ", "") == "#[cfg(test)]":
                pending_cfg = True
            index = end + 1
            continue
        if text[index] == "r" and index + 1 < len(text) and text[index + 1] in {'"', "#"}:
            literal, end, newlines = raw_string(text, index)
            if end < 0:
                break
            consider(path, line, literal, skip_base is not None, findings)
            line += newlines
            index = end
            continue
        if text[index] == '"':
            literal, end, newlines = cooked_string(text, index)
            if end < 0:
                break
            consider(path, line, literal, skip_base is not None, findings)
            line += newlines
            index = end
            continue
        if text[index] == "'":
            end = char_literal_end(text, index)
            if end < 0:
                index += 1
                continue
            line += text[index:end].count("\n")
            index = end
            continue
        char = text[index]
        if skip_base is not None:
            if char == "{":
                brace += 1
                seen_brace = True
            elif char == "}":
                brace -= 1
                if seen_brace and brace == skip_base:
                    skip_base = None
            elif char == ";" and not seen_brace and paren == 0:
                skip_base = None
            elif char == "(":
                paren += 1
            elif char == ")":
                paren = max(0, paren - 1)
            index += 1
            continue
        if char == "{":
            brace += 1
        elif char == "}":
            brace = max(0, brace - 1)
        elif char == "(":
            paren += 1
        elif char == ")":
            paren = max(0, paren - 1)
        elif pending_cfg and not char.isspace():
            skip_base = brace
            seen_brace = False
            pending_cfg = False
            continue
        index += 1
    return findings


def consider(
    path: pathlib.Path,
    line: int,
    literal: str,
    skipped: bool,
    findings: list[str],
) -> None:
    if skipped or not user_facing(literal):
        return
    source = path.read_text(encoding="utf-8").splitlines()
    window = source[line - 1 : line + 1]
    if any("ui-allow:" in item for item in window):
        return
    shown = literal.replace("\n", "\\n")
    findings.append(f"{path.relative_to(ROOT)}:{line}: {shown}")


def user_facing(literal: str) -> bool:
    if IDENT.fullmatch(literal):
        return False
    if literal in PROTOCOL_KINDS:
        return False
    if PASCAL.fullmatch(literal):
        return True
    has_letter = LETTER.search(literal) is not None or CJK.search(literal) is not None
    marked = any(char.isspace() for char in literal) or "=" in literal or CJK.search(literal)
    return has_letter and marked


def matching_bracket(text: str, open_at: int) -> int:
    depth = 0
    index = open_at
    while index < len(text):
        char = text[index]
        if char == "[":
            depth += 1
        elif char == "]":
            depth -= 1
            if depth == 0:
                return index
        elif char == '"':
            _, end, _ = cooked_string(text, index)
            if end < 0:
                return -1
            index = end
            continue
        index += 1
    return -1


def cooked_string(text: str, start: int) -> tuple[str, int, int]:
    chars: list[str] = []
    newlines = 0
    index = start + 1
    while index < len(text):
        char = text[index]
        if char == "\\":
            if index + 1 >= len(text):
                return "", -1, newlines
            escaped = text[index + 1]
            chars.append(escaped)
            if escaped == "\n":
                newlines += 1
            index += 2
            continue
        if char == '"':
            return "".join(chars), index + 1, newlines
        if char == "\n":
            newlines += 1
        chars.append(char)
        index += 1
    return "", -1, newlines


def raw_string(text: str, start: int) -> tuple[str, int, int]:
    index = start + 1
    hashes = 0
    while index < len(text) and text[index] == "#":
        hashes += 1
        index += 1
    if index >= len(text) or text[index] != '"':
        return "", -1, 0
    index += 1
    closer = '"' + ("#" * hashes)
    end = text.find(closer, index)
    if end < 0:
        return "", -1, 0
    literal = text[index:end]
    return literal, end + len(closer), literal.count("\n")


def char_literal_end(text: str, start: int) -> int:
    index = start + 1
    if index < len(text) and text[index] == "\\":
        index += 2
    elif index < len(text):
        index += 1
    if index < len(text) and text[index] == "'":
        return index + 1
    return -1


if __name__ == "__main__":
    sys.exit(main())
