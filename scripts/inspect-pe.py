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
def dll_name(rva):
    name = offset(rva)
    return data[name:data.index(0, name)].decode("ascii")

import_rva = u32(optional + 112 + 8)
if import_rva:
    descriptor = offset(import_rva)
    while any(data[descriptor:descriptor + 20]):
        imports.append(dll_name(u32(descriptor + 12)))
        descriptor += 20
# Delay-loaded graphics libraries must not bypass the normal import-table check.
if u32(optional + 108) > 13:
    delay_rva = u32(optional + 112 + 13 * 8)
    if delay_rva:
        descriptor = offset(delay_rva)
        while any(data[descriptor:descriptor + 32]):
            assert u32(descriptor) & 1, "Unsupported VA-based delay import descriptor"
            imports.append(dll_name(u32(descriptor + 4)))
            descriptor += 32
assert not any(n.lower().startswith(("vcruntime", "msvcp")) for n in imports), "External MSVC redistributable dependency"
forbidden = {"opengl32.dll", "libegl.dll", "libglesv2.dll", "d3d9.dll", "d3d11.dll", "d3d12.dll", "dxgi.dll", "vulkan-1.dll"}
assert not (set(map(str.lower, imports)) & forbidden), "GPU/OpenGL presentation dependency"
assert "gdi32.dll" in set(map(str.lower, imports)), "Missing Win32 GDI dependency"
print(json.dumps({"path": str(path.resolve()), "architecture": "x64", "subsystem": "Windows GUI",
                  "size_bytes": len(data), "imports": sorted(imports)}, indent=2))
