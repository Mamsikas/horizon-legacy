"""Observable PE parsing and Win7 policy checks, with standalone synthetic PE files."""

import importlib.util
import json
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOL = Path(__file__).resolve().parents[1] / "tools" / "check_win7_imports.py"
SPEC = importlib.util.spec_from_file_location("check_win7_imports", TOOL)
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


def fixture(dll="kernel32.dll", symbol="GetLastError", *, x86=False, delayed=False,
            legacy_delay=False):
    """A single-section PE, with independently chosen header/table/name locations."""
    data = bytearray(0x1200)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    size = 0xE0 if x86 else 0xF0
    struct.pack_into("<HH12xH", data, 0x84, 0x14C if x86 else 0x8664, 1, size)
    optional = 0x98
    struct.pack_into("<H", data, optional, 0x10B if x86 else 0x20B)
    base = 0x400000 if x86 else 0x140000000
    struct.pack_into("<I" if x86 else "<Q", data, optional + (28 if x86 else 24), base)
    struct.pack_into("<HH", data, optional + 40, 6, 1)
    struct.pack_into("<HH", data, optional + 48, 6, 1)
    struct.pack_into("<I", data, optional + 60, 0x200)
    struct.pack_into("<H", data, optional + 68, 2)
    directory = optional + (96 if x86 else 112)
    struct.pack_into("<I", data, directory - 4, 16)
    section = optional + size
    data[section:section + 8] = b".idata\0\0"
    struct.pack_into("<IIII", data, section + 8, 0x2000, 0x1000, 0x1000, 0x200)
    table_width = 32 if delayed else 20
    struct.pack_into("<II", data, directory + 8 * (13 if delayed else 1),
                     0x1000, table_width * 2)
    # File offsets 0x300 / 0x400 / 0x500 map to RVAs 0x1100 / 0x1200 / 0x1300.
    data[0x300:0x301 + len(dll)] = dll.encode() + b"\0"
    struct.pack_into("<H", data, 0x500, 7)
    if isinstance(symbol, int):
        value = (1 << (31 if x86 else 63)) | symbol
    else:
        data[0x502:0x503 + len(symbol)] = symbol.encode() + b"\0"
        value = 0x1300 + (base if legacy_delay else 0)
    struct.pack_into("<I" if x86 else "<Q", data, 0x400, value)
    if delayed:
        delta = base if legacy_delay else 0
        struct.pack_into("<8I", data, 0x200, 0 if legacy_delay else 1,
                         0x1100 + delta, 0, 0x1200 + delta, 0x1200 + delta, 0, 0, 0)
    else:
        struct.pack_into("<5I", data, 0x200, 0x1200, 0, 0, 0x1100, 0x1200)
    return data


class Win7Imports(unittest.TestCase):
    def test_reads_named_load_and_delay_imports(self):
        for delayed in (False, True):
            with self.subTest(delayed=delayed):
                report = CHECK.audit(fixture(delayed=delayed))
                self.assertTrue(report["passed"], report["problems"])
                self.assertEqual(report["imports"], [{"dll": "kernel32.dll",
                    "symbols": ["GetLastError"], "delayed": delayed}])

    def test_pe32_and_legacy_delay_addresses(self):
        for legacy in (False, True):
            report = CHECK.audit(fixture(x86=True, delayed=True, legacy_delay=legacy), "x86")
            self.assertTrue(report["passed"], report["problems"])
            self.assertEqual(report["imports"][0]["symbols"], ["GetLastError"])

    def test_rejects_wrong_architecture_and_newer_subsystem(self):
        self.assertFalse(CHECK.audit(fixture(x86=True))["passed"])
        data = fixture()
        struct.pack_into("<HH", data, 0x98 + 48, 10, 0)
        self.assertFalse(CHECK.audit(data)["passed"])

    def test_rejects_post_win7_api_in_both_import_tables(self):
        for delayed in (False, True):
            report = CHECK.audit(fixture(symbol="WaitOnAddress", delayed=delayed))
            self.assertFalse(report["passed"])
            self.assertIn("post-Win7 API", report["problems"][0])

    def test_rejects_unreviewed_symbols_dlls_and_ordinals(self):
        for dll, symbol in (("kernel32.dll", "NewUnreviewedAPI"),
                            ("combase.dll", "CoTaskMemFree"),
                            ("api-ms-win-core-synch-l1-2-0.dll", "WaitOnAddress"),
                            ("d3d11.dll", "D3D11CreateDevice"), ("kernel32.dll", 15)):
            with self.subTest(dll=dll, symbol=symbol):
                self.assertFalse(CHECK.audit(fixture(dll, symbol))["passed"])

    def test_accepts_import_address_table_when_original_table_is_zero(self):
        data = fixture()
        struct.pack_into("<I", data, 0x200, 0)
        self.assertTrue(CHECK.audit(data)["passed"])

    def test_rejects_short_files_and_out_of_bounds_rvas(self):
        for cut in (0, 2, 63, 0x85, 0xA0, 0x1FF, 0x1100):
            with self.subTest(cut=cut), self.assertRaises(CHECK.PEError):
                CHECK.audit(fixture()[:cut])
        data = fixture()
        struct.pack_into("<Q", data, 0x400, 0x2400)  # virtual section tail has no bytes
        with self.assertRaisesRegex(CHECK.PEError, "file-backed"):
            CHECK.audit(data)

    def test_rejects_unterminated_descriptor_thunk_and_name(self):
        data = fixture()
        struct.pack_into("<I", data, 0x98 + 112 + 8 + 4, 20)
        with self.assertRaisesRegex(CHECK.PEError, "descriptor"):
            CHECK.audit(data)
        data = fixture()
        data[0x400:] = b"\xff" * (len(data) - 0x400)
        with self.assertRaisesRegex(CHECK.PEError, "thunk"):
            CHECK.audit(data)
        data = fixture()
        struct.pack_into("<Q", data, 0x400, 0x1FF0)
        data[0x11F2:] = b"A" * 14
        with self.assertRaisesRegex(CHECK.PEError, "unterminated"):
            CHECK.audit(data)

    def test_rejects_overlapping_sections_and_bad_directory_counts(self):
        data = fixture()
        struct.pack_into("<I", data, 0x98 + 112 - 4, 17)
        with self.assertRaisesRegex(CHECK.PEError, "directory"):
            CHECK.audit(data)
        data = fixture()
        struct.pack_into("<H", data, 0x86, 2)
        section = 0x98 + 0xF0
        struct.pack_into("<IIII", data, section + 40 + 8, 0x1000, 0x1800, 0, 0)
        with self.assertRaisesRegex(CHECK.PEError, "overlapping"):
            CHECK.audit(data)

    def test_cli_json_reports_audit_failure_and_continues_other_files(self):
        with tempfile.TemporaryDirectory() as scratch:
            good = Path(scratch) / "good.exe"
            bad = Path(scratch) / "bad.exe"
            good.write_bytes(fixture())
            bad.write_bytes(b"not PE")
            result = subprocess.run([sys.executable, str(TOOL), "--json", str(good), str(bad)],
                                    capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 1)
            reports = json.loads(result.stdout)
            self.assertTrue(reports[0]["passed"])
            self.assertFalse(reports[1]["passed"])
            self.assertIn("hardware", reports[0]["scope"])


if __name__ == "__main__":
    unittest.main()
