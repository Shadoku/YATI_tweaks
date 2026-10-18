#!/usr/bin/env python3
"""Sanity-check a built native importer library before it is published (native.yml).

A library whose bytes were mangled after the link (a workflow that merges several
build artifacts overlapping on the same file into one directory can write a fresh
library over a stale one of another length and keep the stale one's tail) fails
to load: "Error 1114: a DLL initialization routine failed" on Windows, a crash
inside the dynamic loader on Linux. Such an image betrays itself at its ends. A
PE file must end where its last section ends and, with --built-after (seconds
since the epoch; the job's start), be linked no earlier than that. An ELF file
must hold its section header table within the file, ending at its last byte,
and have a dynamic section. A Mach-O file (a universal binary or a single
slice) must carry a Mach-O magic. Anything else only has to be non-empty.

    python3 -I tools/check_native_image.py --built-after $(date +%s) path/to/library
"""
import argparse
import struct
import sys
import time


def check_pe(data: bytes, built_after: float, path: str) -> list[str]:
    problems: list[str] = []
    e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
    if data[e_lfanew:e_lfanew + 4] != b"PE\0\0":
        return [f"{path}: not a PE image"]
    sections, timestamp, _, _, size_of_optional = struct.unpack_from("<HIIIH", data, e_lfanew + 6)
    table = e_lfanew + 24 + size_of_optional
    end = 0
    for i in range(sections):
        _, _, _, raw_size, raw_offset = struct.unpack_from("<8sIIII", data, table + 40 * i)
        end = max(end, raw_offset + raw_size)
    certificate = struct.unpack_from("<II", data, e_lfanew + 24 + 144)  # the security directory
    if certificate[1]:
        end = max(end, certificate[0] + certificate[1])
    if len(data) > end:
        problems.append(f"{path}: {len(data) - end} bytes past the last section (an incremental link on a stale output?)")
    when = time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime(timestamp))
    print(f"{path}: PE, {sections} sections, {len(data)} bytes, linked {when}")
    if built_after and timestamp < built_after - 300:
        problems.append(f"{path}: linked {when}, before this build started (an incremental link on a stale output?)")
    return problems


def check_elf(data: bytes, path: str) -> list[str]:
    problems: list[str] = []
    if data[4] != 2 or data[5] != 1:
        return [f"{path}: not a 64-bit little-endian ELF"]
    e_shoff = struct.unpack_from("<Q", data, 0x28)[0]
    e_shentsize, e_shnum = struct.unpack_from("<HH", data, 0x3A)
    table_end = e_shoff + e_shentsize * e_shnum
    print(f"{path}: ELF, {e_shnum} section headers at {e_shoff}, {len(data)} bytes")
    if e_shoff == 0 or table_end > len(data):
        problems.append(f"{path}: section header table runs past the end of the file")
    elif table_end != len(data):
        problems.append(f"{path}: {len(data) - table_end} bytes past the section header table")
    has_dynamic = False
    if table_end <= len(data):
        for i in range(e_shnum):
            sh_type = struct.unpack_from("<I", data, e_shoff + i * e_shentsize + 4)[0]
            if sh_type == 6:  # SHT_DYNAMIC
                has_dynamic = True
    if not has_dynamic:
        problems.append(f"{path}: no dynamic section")
    return problems


MACHO_MAGICS = {b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--built-after", type=float, default=0.0, help="seconds since the epoch the library must be linked after")
    parser.add_argument("paths", nargs="+")
    args = parser.parse_args()
    problems: list[str] = []
    for path in args.paths:
        with open(path, "rb") as handle:
            data = handle.read()
        if not data:
            problems.append(f"{path}: empty")
        elif data[:2] == b"MZ":
            problems.extend(check_pe(data, args.built_after, path))
        elif data[:4] == b"\x7fELF":
            problems.extend(check_elf(data, path))
        elif data[:4] in MACHO_MAGICS:
            print(f"{path}: Mach-O, {len(data)} bytes")
        else:
            print(f"{path}: {len(data)} bytes")
    for problem in problems:
        print(f"error: {problem}", file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
