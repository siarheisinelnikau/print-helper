"""Read the packaged PE import table without executing the application."""
from pathlib import Path
import json
import struct
import sys

path = Path(sys.argv[1])
data = path.read_bytes()
u16 = lambda offset: struct.unpack_from("<H", data, offset)[0]
u32 = lambda offset: struct.unpack_from("<I", data, offset)[0]
assert data[:2] == b"MZ"
pe = u32(60)
assert data[pe:pe + 4] == b"PE\0\0"
assert u16(pe + 4) == 0x8664, "Expected x64 executable"
optional = pe + 24
assert u16(optional) == 0x20B, "Expected PE32+"
assert u16(optional + 68) == 2, "Expected Windows GUI subsystem"
sections = pe + 24 + u16(pe + 20)

def offset(rva):
    if rva < u32(optional + 60):
        return rva
    for i in range(u16(pe + 6)):
        section = sections + i * 40
        base = u32(section + 12)
        size = max(u32(section + 8), u32(section + 16))
        if base <= rva < base + size:
            return rva - base + u32(section + 20)
    raise ValueError(f"Unmapped RVA {rva}")

imports = []
descriptor = offset(u32(optional + 112 + 8))
while any(data[descriptor:descriptor + 20]):
    name = offset(u32(descriptor + 12))
    imports.append(data[name:data.index(0, name)].decode("ascii"))
    descriptor += 20
assert not any(n.lower().startswith(("vcruntime", "msvcp")) for n in imports), "External MSVC redistributable dependency"
print(json.dumps({"path": str(path.resolve()), "architecture": "x64", "subsystem": "Windows GUI",
                  "size_bytes": len(data), "imports": sorted(imports)}, indent=2))
