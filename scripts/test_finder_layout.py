"""Checks the Finder records embedded in the release disk image."""

import plistlib
import struct
import unittest

from finder_layout import BLOCK_SIZE, NODE_OFFSET, build_store


class FinderLayoutTests(unittest.TestCase):
    def test_window_and_icon_records(self):
        store = build_store()
        self.assertEqual(len(store), BLOCK_SIZE)
        self.assertEqual(store[4:8], b"Bud1")
        self.assertEqual(struct.unpack_from(">I", store, 0x14)[0], 0x200C)
        self.assertEqual(store[0x1411:0x1415], b"DSDB")

        offset = NODE_OFFSET + 12
        count = struct.unpack_from(">I", store, NODE_OFFSET + 8)[0]
        found = {}
        for _ in range(count):
            length = struct.unpack_from(">I", store, offset)[0]
            offset += 4
            name = store[offset : offset + length * 2].decode("utf-16-be")
            offset += length * 2
            code = store[offset : offset + 4].decode("ascii")
            kind = store[offset + 4 : offset + 8]
            self.assertEqual(kind, b"blob")
            offset += 8
            size = struct.unpack_from(">I", store, offset)[0]
            offset += 4
            found[name, code] = store[offset : offset + size]
            offset += size

        self.assertEqual(count, 4)
        window = plistlib.loads(found[".", "bwsp"])
        icons = plistlib.loads(found[".", "icvp"])
        self.assertEqual(window["WindowBounds"], "{{420, 250}, {720, 440}}")
        self.assertFalse(window["ShowToolbar"])
        self.assertEqual(icons["arrangeBy"], "none")
        self.assertEqual(icons["backgroundType"], 1)
        self.assertEqual(struct.unpack_from(">II", found["Koi.app", "Iloc"]), (180, 195))
        self.assertEqual(
            struct.unpack_from(">II", found["Applications", "Iloc"]), (540, 195)
        )


if __name__ == "__main__":
    unittest.main()
