#!/usr/bin/env python3
"""Dependency-free static checks for the IMECurRust source package."""

from __future__ import annotations

import ctypes
import re
import struct
import tomllib
import wave
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"


def check_delimiters(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    stack: list[tuple[str, int]] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    state = "code"
    block_depth = 0
    line = 1
    index = 0

    def looks_like_char_literal(position: int) -> bool:
        # Rust lifetimes such as 'a are not character literals.
        if position + 2 < len(text) and text[position + 2] == "'":
            return True
        return (
            position + 3 < len(text)
            and text[position + 1] == "\\"
            and text[position + 3] == "'"
        )

    while index < len(text):
        char = text[index]
        next_char = text[index + 1] if index + 1 < len(text) else ""

        if state == "code":
            if char == "/" and next_char == "/":
                state = "line-comment"
                index += 2
                continue
            if char == "/" and next_char == "*":
                state = "block-comment"
                block_depth = 1
                index += 2
                continue
            if char == "r" and re.match(r'r#*"', text[index:]):
                raise AssertionError(f"{path}: raw string scanner update required at line {line}")
            if char == '"':
                state = "string"
                index += 1
                continue
            if char == "'" and looks_like_char_literal(index):
                state = "char"
                index += 1
                continue
            if char in "([{":
                stack.append((char, line))
            elif char in ")]}":
                if not stack or stack[-1][0] != pairs[char]:
                    raise AssertionError(f"{path}: mismatched {char!r} at line {line}")
                stack.pop()
            if char == "\n":
                line += 1
            index += 1
            continue

        if state == "line-comment":
            if char == "\n":
                state = "code"
                line += 1
            index += 1
            continue

        if state == "block-comment":
            if char == "/" and next_char == "*":
                block_depth += 1
                index += 2
                continue
            if char == "*" and next_char == "/":
                block_depth -= 1
                index += 2
                if block_depth == 0:
                    state = "code"
                continue
            if char == "\n":
                line += 1
            index += 1
            continue

        if state in {"string", "char"}:
            if char == "\\":
                index += 2
                continue
            terminator = '"' if state == "string" else "'"
            if char == terminator:
                state = "code"
            if char == "\n":
                line += 1
            index += 1

    if stack:
        raise AssertionError(f"{path}: unclosed delimiters: {stack[-5:]}")
    if state not in {"code", "line-comment"}:
        raise AssertionError(f"{path}: source ended in state {state}")


def extract_asset(name: str, source: str) -> bytes:
    match = re.search(
        rf"pub const {re.escape(name)}: &str = concat!\((.*?)\);",
        source,
        re.DOTALL,
    )
    if match is None:
        raise AssertionError(f"asset not found: {name}")
    hex_text = "".join(re.findall(r'"([0-9A-Fa-f]+)"', match.group(1)))
    return bytes.fromhex(hex_text)


@dataclass(frozen=True)
class Point:
    x: int
    y: int


@dataclass(frozen=True)
class Rect:
    left: int
    top: int
    right: int
    bottom: int


def center(rect: Rect) -> Point:
    return Point(
        rect.left + (rect.right - rect.left) // 2,
        rect.top + (rect.bottom - rect.top) // 2,
    )


def nearest_edge(point: Point, monitor: Rect) -> str:
    distances = [
        ("bottom", abs(monitor.bottom - point.y)),
        ("top", abs(point.y - monitor.top)),
        ("left", abs(point.x - monitor.left)),
        ("right", abs(monitor.right - point.x)),
    ]
    return min(distances, key=lambda item: item[1])[0]


def taskbar_edge(reference: Point, monitor: Rect, work: Rect) -> str:
    if work.bottom < monitor.bottom and reference.y >= work.bottom:
        return "bottom"
    if work.top > monitor.top and reference.y < work.top:
        return "top"
    if work.left > monitor.left and reference.x < work.left:
        return "left"
    if work.right < monitor.right and reference.x >= work.right:
        return "right"
    return nearest_edge(reference, monitor)


def clamp(value: int, minimum: int, maximum: int) -> int:
    return max(minimum, min(value, maximum))


def placement(icon: Rect | None, fallback: Point, monitor: Rect, work: Rect) -> tuple[Point, str, str]:
    reference = center(icon) if icon else fallback
    edge = taskbar_edge(reference, monitor, work)
    middle_x = work.left + (work.right - work.left) // 2
    middle_y = work.top + (work.bottom - work.top) // 2

    if edge == "bottom":
        horizontal = "right" if reference.x >= middle_x else "left"
        raw_x = (icon.right if horizontal == "right" else icon.left) if icon else reference.x
        raw_y = min(icon.top, work.bottom) if icon else work.bottom
        return Point(clamp(raw_x, work.left, work.right), clamp(raw_y, work.top, work.bottom)), horizontal, "bottom"
    if edge == "top":
        horizontal = "right" if reference.x >= middle_x else "left"
        raw_x = (icon.right if horizontal == "right" else icon.left) if icon else reference.x
        raw_y = max(icon.bottom, work.top) if icon else work.top
        return Point(clamp(raw_x, work.left, work.right), clamp(raw_y, work.top, work.bottom)), horizontal, "top"
    if edge == "right":
        vertical = "bottom" if reference.y >= middle_y else "top"
        raw_x = min(icon.left, work.right) if icon else work.right
        raw_y = (icon.bottom if vertical == "bottom" else icon.top) if icon else reference.y
        return Point(clamp(raw_x, work.left, work.right), clamp(raw_y, work.top, work.bottom)), "right", vertical

    vertical = "bottom" if reference.y >= middle_y else "top"
    raw_x = max(icon.right, work.left) if icon else work.left
    raw_y = (icon.bottom if vertical == "bottom" else icon.top) if icon else reference.y
    return Point(clamp(raw_x, work.left, work.right), clamp(raw_y, work.top, work.bottom)), "left", vertical


def check_struct_sizes() -> None:
    class CPoint(ctypes.Structure):
        _fields_ = [("x", ctypes.c_int32), ("y", ctypes.c_int32)]

    class CRect(ctypes.Structure):
        _fields_ = [
            ("left", ctypes.c_int32),
            ("top", ctypes.c_int32),
            ("right", ctypes.c_int32),
            ("bottom", ctypes.c_int32),
        ]

    class CGuid(ctypes.Structure):
        _fields_ = [
            ("Data1", ctypes.c_uint32),
            ("Data2", ctypes.c_uint16),
            ("Data3", ctypes.c_uint16),
            ("Data4", ctypes.c_uint8 * 8),
        ]

    class CMonitorInfo(ctypes.Structure):
        _fields_ = [
            ("cbSize", ctypes.c_uint32),
            ("rcMonitor", CRect),
            ("rcWork", CRect),
            ("dwFlags", ctypes.c_uint32),
        ]

    class CTpmParams(ctypes.Structure):
        _fields_ = [("cbSize", ctypes.c_uint32), ("rcExclude", CRect)]

    class CNotifyIconIdentifier(ctypes.Structure):
        _fields_ = [
            ("cbSize", ctypes.c_uint32),
            ("hWnd", ctypes.c_void_p),
            ("uID", ctypes.c_uint32),
            ("guidItem", CGuid),
        ]

    assert ctypes.sizeof(CPoint) == 8
    assert ctypes.sizeof(CRect) == 16
    assert ctypes.sizeof(CGuid) == 16
    assert ctypes.sizeof(CMonitorInfo) == 40
    assert ctypes.sizeof(CTpmParams) == 20
    if ctypes.sizeof(ctypes.c_void_p) == 8:
        assert ctypes.sizeof(CNotifyIconIdentifier) == 40
    else:
        assert ctypes.sizeof(CNotifyIconIdentifier) == 28

    # Explicit packed size check for the pointer-free structures on either ABI.
    assert struct.calcsize("<IiiiiiiiiI") == 40
    assert struct.calcsize("<Iiiii") == 20


def main() -> None:
    with (ROOT / "Cargo.toml").open("rb") as handle:
        cargo = tomllib.load(handle)
    assert cargo["package"]["name"] == "ime-cursor"
    assert cargo["package"]["version"] == "1.0.2"
    assert 'version = "1.0.2"' in (ROOT / "Cargo.lock").read_text(encoding="utf-8")

    for rust_file in sorted(SRC.glob("*.rs")):
        check_delimiters(rust_file)

    main_rs = (SRC / "main.rs").read_text(encoding="utf-8")
    win_rs = (SRC / "win.rs").read_text(encoding="utf-8")
    assert 'const APP_VERSION: &str = "1.0.2";' in main_rs

    for symbol in [
        "NOTIFYICONIDENTIFIER",
        "MONITORINFO",
        "TPMPARAMS",
        "MonitorFromPoint",
        "GetMonitorInfoW",
        "TrackPopupMenuEx",
        "Shell_NotifyIconGetRect",
        "HWND_TOPMOST",
        "HWND_NOTOPMOST",
    ]:
        assert symbol in win_rs, symbol
        assert symbol in main_rs or symbol in {"NOTIFYICONIDENTIFIER", "MONITORINFO", "TPMPARAMS"}, symbol

    for token in [
        "tray_icon_rect",
        "tray_menu_placement",
        "calculate_tray_menu_placement",
        "taskbar_or_icon_exclusion",
        "point_from_message",
        "TrackPopupMenuEx(",
        "SetForegroundWindow(self.main_hwnd)",
        "NIM_SETFOCUS",
    ]:
        assert token in main_rs, token
    assert "let command = TrackPopupMenu(" not in main_rs

    # Strict text-cursor gating: cursor classification must happen before the IME query,
    # and unknown/application-defined cursors must not receive the previous focus-based badge.
    timer_start = main_rs.index("unsafe fn on_timer(&mut self)")
    timer_end = main_rs.index("unsafe fn show_ime", timer_start)
    timer_body = main_rs[timer_start:timer_end]
    gate = timer_body.index("current_cursor_class() != CurrentCursorClass::IBeam")
    query = timer_body.index("self.ime_engine.query")
    assert gate < query
    assert "self.hide_badge();" in timer_body[:query]
    assert "return;" in timer_body[:query]
    assert "self.was_text_cursor = true;" in timer_body
    assert "self.force_cursor_refresh = true;" in timer_body
    assert "self.old_kind = None;" not in timer_body
    assert "|| self.force_cursor_refresh" in main_rs
    assert "self.force_cursor_refresh = false;" in main_rs
    assert "focused_root_window" not in main_rs
    badge_start = main_rs.index("unsafe fn should_show_fallback_badge")
    badge_end = main_rs.index("unsafe fn hide_badge", badge_start)
    badge_body = main_rs[badge_start:badge_end]
    assert "CurrentCursorClass::IBeam && !self.cursor_apply_ok" in badge_body
    assert "CurrentCursorClass::Unknown =>" not in badge_body

    assets = (SRC / "assets.rs").read_text(encoding="utf-8")
    for name in [
        "CURSOR_DEFAULT_HEX",
        "CURSOR_EL_HEX",
        "CURSOR_EU_HEX",
        "CURSOR_JH_HEX",
        "CURSOR_JK_HEX",
        "CURSOR_K_HEX",
    ]:
        assert len(extract_asset(name, assets)) == 128, name
    for name in ["ICON_DEFAULT_HEX", "ICON_E_HEX", "ICON_J_HEX", "ICON_K_HEX"]:
        assert len(extract_asset(name, assets)) == 296, name

    check_struct_sizes()

    scenarios = [
        # Bottom taskbar: menu bottom is clamped to the work-area boundary.
        (
            Rect(1850, 1048, 1874, 1072),
            Point(0, 0),
            Rect(0, 0, 1920, 1080),
            Rect(0, 0, 1920, 1040),
            (Point(1874, 1040), "right", "bottom"),
        ),
        # Top taskbar on a monitor with negative X coordinates.
        (
            Rect(-80, 10, -56, 34),
            Point(0, 0),
            Rect(-1600, 0, 0, 900),
            Rect(-1600, 48, 0, 900),
            (Point(-56, 48), "right", "top"),
        ),
        # Left and right taskbars.
        (
            Rect(10, 830, 34, 854),
            Point(0, 0),
            Rect(0, 0, 1600, 900),
            Rect(52, 0, 1600, 900),
            (Point(52, 854), "left", "bottom"),
        ),
        (
            Rect(-42, 830, -18, 854),
            Point(0, 0),
            Rect(-1600, 0, 0, 900),
            Rect(-1600, 0, -52, 900),
            (Point(-52, 854), "right", "bottom"),
        ),
        # Overflow flyout icon: use the icon itself instead of the taskbar strip.
        (
            Rect(950, 900, 974, 924),
            Point(0, 0),
            Rect(0, 0, 1920, 1080),
            Rect(0, 0, 1920, 1040),
            (Point(974, 900), "right", "bottom"),
        ),
    ]
    for icon, fallback, monitor, work, expected in scenarios:
        actual = placement(icon, fallback, monitor, work)
        assert actual == expected, (actual, expected)

    # GET_X/Y_LPARAM-equivalent sign extension for a negative-coordinate monitor.
    packed = ((-120 & 0xFFFF) << 16) | (-640 & 0xFFFF)
    signed_x = ctypes.c_int16(packed & 0xFFFF).value
    signed_y = ctypes.c_int16((packed >> 16) & 0xFFFF).value
    assert (signed_x, signed_y) == (-640, -120)

    for name in ["IMEE.wav", "IMEJ.wav", "IMEK.wav"]:
        path = ROOT / "assets" / name
        with wave.open(str(path), "rb") as audio:
            assert audio.getnchannels() >= 1
            assert audio.getframerate() > 0
            assert audio.getnframes() > 0

    print("IMECurRust 1.0.2 static checks passed")


if __name__ == "__main__":
    main()
