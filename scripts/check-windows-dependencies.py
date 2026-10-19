"""Release gate: inspect PE imports without loading code or changing the system."""
from pathlib import Path
import argparse
import json
import struct


def imports(path):
    data = path.read_bytes()
    assert data[:2] == b"MZ", f"Not a PE file: {path}"
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    assert data[pe:pe + 4] == b"PE\0\0"
    coff = pe + 4
    count = struct.unpack_from("<H", data, coff + 2)[0]
    size = struct.unpack_from("<H", data, coff + 16)[0]
    optional = coff + 20
    magic = struct.unpack_from("<H", data, optional)[0]
    directory = optional + (112 if magic == 0x20B else 96)
    rva = struct.unpack_from("<I", data, directory + 8)[0]
    sections = []
    for index in range(count):
        virtual_size, address, raw_size, raw = struct.unpack_from("<IIII", data, optional + size + 40 * index + 8)
        sections.append((address, max(virtual_size, raw_size), raw))

    def offset(address):
        for start, length, raw in sections:
            if start <= address < start + length:
                return raw + address - start
        return address

    result = []
    position = offset(rva)
    while rva and any(data[position:position + 20]):
        name = offset(struct.unpack_from("<I", data, position + 12)[0])
        result.append(data[name:data.index(b"\0", name)].decode("ascii"))
        position += 20
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", nargs="+", type=Path)
    arguments = parser.parse_args()
    report = {}
    for file in arguments.files:
        dependencies = imports(file)
        unexpected = [name for name in dependencies if name.lower().startswith(("msvcp", "vcruntime", "msvcr"))]
        assert not unexpected, f"{file}: needs a separate Visual C++ runtime: {unexpected}"
        report[str(file)] = dependencies
    print(json.dumps(report, indent=2))
