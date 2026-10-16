"""Create the Finder icon layout for Koi's read-only disk image."""

import plistlib
import struct
import sys
from pathlib import Path


WINDOW_WIDTH = 720
WINDOW_HEIGHT = 440
BLOCK_SIZE = 0x3000
NODE_OFFSET = 0x2000


def integer(value):
    return struct.pack(">I", value)


def record(name, code, kind, value):
    encoded_name = name.encode("utf-16-be")
    result = integer(len(encoded_name) // 2) + encoded_name
    result += code.encode("ascii") + kind.encode("ascii")
    if kind == "blob":
        result += integer(len(value)) + value
    elif kind == "type":
        result += value
    else:
        raise ValueError(f"Unsupported Finder record: {kind}")
    return result


def icon_location(x, y):
    return struct.pack(">II", x, y) + b"\xff" * 8


def layout_records():
    window = {
        "ShowStatusBar": False,
        "ShowToolbar": False,
        "ShowTabView": False,
        "ContainerShowSidebar": False,
        "ShowSidebar": False,
        "WindowBounds": f"{{{{420, 250}}, {{{WINDOW_WIDTH}, {WINDOW_HEIGHT}}}}}",
    }
    icons = {
        "viewOptionsVersion": 1,
        "arrangeBy": "none",
        "backgroundType": 1,
        "backgroundColorRed": 0.93,
        "backgroundColorGreen": 0.94,
        "backgroundColorBlue": 0.95,
        "iconSize": 96.0,
        "textSize": 14.0,
        "gridSpacing": 40.0,
        "gridOffsetX": 0.0,
        "gridOffsetY": 0.0,
        "labelOnBottom": True,
        "showIconPreview": True,
        "showItemInfo": False,
    }
    entries = [
        (".", "bwsp", "blob", plistlib.dumps(window, fmt=plistlib.FMT_BINARY)),
        (".", "icvp", "blob", plistlib.dumps(icons, fmt=plistlib.FMT_BINARY)),
        ("Applications", "Iloc", "blob", icon_location(540, 195)),
        ("Koi.app", "Iloc", "blob", icon_location(180, 195)),
    ]
    return [record(*entry) for entry in entries]


def build_store():
    entries = layout_records()
    node = integer(0) + integer(0) + integer(len(entries)) + b"".join(entries)
    if len(node) > 0x1000:
        raise ValueError("Finder layout exceeds its node")

    store = bytearray(BLOCK_SIZE)
    struct.pack_into(">I4sIIII", store, 0, 1, b"Bud1", 0x1000, 0x800, 0x1000, 0x200C)
    struct.pack_into(">II", store, 0x2C, 0x800, 0x800)
    struct.pack_into(">6I", store, 0x40, 0, 2, 0, len(entries), 1, 0x1000)
    struct.pack_into(">4I", store, 0x800, 0, 1, 0, 0x80B)
    store[NODE_OFFSET : NODE_OFFSET + len(node)] = node

    struct.pack_into(">6I", store, 0x1000, 0, 3, 0, 0x100B, 0x45, 0x200C)
    struct.pack_into(">I", store, 0x140C, 1)
    store[0x1410:0x1415] = b"\x04DSDB"
    struct.pack_into(">I", store, 0x1415, 1)
    return bytes(store)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: finder_layout.py OUTPUT/.DS_Store")
    Path(sys.argv[1]).write_bytes(build_store())
