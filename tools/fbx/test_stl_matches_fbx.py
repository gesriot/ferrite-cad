#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The triangle comparison must reject incomplete and non-finite evidence."""
import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest


class Comparison(unittest.TestCase):
    def test_oriented_geometry_and_invalid_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            stl, report = root / "body.stl", root / "reader.txt"
            header = bytes(80) + struct.pack("<I", 1)
            vertices = [0., 0., 0., 1., 0., 0., 0., 1., 0.]
            def binary(points):
                return header + struct.pack("<12fH", 0., 0., 1., *points, 0)
            good_stl = binary(vertices)
            triangle = "FCAD_TRIANGLE 0 0 0 .001 0 0 0 0 -.001\n"
            summary = "FCAD_TRIANGLE_SUMMARY instances=1 triangles=1\n"
            result = "FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=6 failures=0\n"
            good_report = triangle + summary + result
            cases = [
                ("equal", good_stl, good_report, True),
                ("cyclic", good_stl, "FCAD_TRIANGLE .001 0 0 0 0 -.001 0 0 0\n" + summary + result, True),
                ("reversed", good_stl, "FCAD_TRIANGLE 0 0 0 0 0 -.001 .001 0 0\n" + summary + result, False),
                ("moved", good_stl, good_report.replace(".001 0", ".002 0", 1), False),
                ("nan reader", good_stl, "FCAD_TRIANGLE " + " ".join(["nan"] * 9) + "\n" + summary + result, False),
                ("infinite reader", good_stl, good_report.replace(".001 0", "inf 0", 1), False),
                ("nan STL", binary([float("nan")] * 9), good_report, False),
                ("short STL", b"", good_report, False),
                ("empty", bytes(84), summary.replace("triangles=1", "triangles=0") + result, False),
                ("missing result", good_stl, triangle + summary, False),
                ("reader failed", good_stl, good_report.replace("failures=0", "failures=1"), False),
                ("wrong summary", good_stl, good_report.replace("triangles=1", "triangles=2"), False),
                ("duplicate summary", good_stl, good_report + summary, False),
            ]
            for name, data, text, accepted in cases:
                with self.subTest(name=name):
                    stl.write_bytes(data)
                    report.write_text(text)
                    process = subprocess.run([sys.executable,
                        str(pathlib.Path(__file__).with_name("stl-matches-fbx.py")),
                        str(stl), str(report)], capture_output=True, text=True)
                    self.assertEqual(process.returncode == 0, accepted,
                                     process.stdout + process.stderr)
                    if not accepted:
                        self.assertNotIn("FCAD_STL_FBX_MATCH", process.stdout)


if __name__ == "__main__":
    unittest.main()
