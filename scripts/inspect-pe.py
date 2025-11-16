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

# Check the actual linked icon resources against the source ICO, not just its filename.
resource_rva = u32(optional + 112 + 2 * 8)
assert resource_rva, "Missing executable resources"
resource_root = offset(resource_rva)


def directory(relative):
    base = resource_root + relative
    return [(u32(base + 16 + i*8), u32(base + 20 + i*8))
            for i in range(u16(base + 12) + u16(base + 14))]


def resource_images(kind):
    type_entry = next(value for key, value in directory(0) if key == kind)
    assert type_entry & 0x80000000, "Expected a resource type directory"
    images = {}
    for key, value in directory(type_entry & 0x7fffffff):
        assert value & 0x80000000, "Expected a language directory"
        languages = directory(value & 0x7fffffff)
        assert len(languages) == 1, "Unexpected resource languages"
        entry = resource_root + languages[0][1]
        start = offset(u32(entry))
        images[key] = data[start:start + u32(entry + 4)]
    return images


ico = (Path(__file__).resolve().parent.parent / "assets/app-icon.ico").read_bytes()
group = resource_images(14)[1]  # RT_GROUP_ICON, also loaded by the native window.
icons = resource_images(3)  # RT_ICON
assert group[:6] == ico[:6], "Missing icon resolutions"
icon_sizes = []
for i in range(struct.unpack_from("<H", ico, 4)[0]):
    entry = ico[6 + i*16:22 + i*16]
    linked = group[6 + i*14:20 + i*14]
    assert linked[:12] == entry[:12], "Icon metadata changed during linking"
    length, start = struct.unpack_from("<II", entry, 8)
    ordinal = struct.unpack_from("<H", linked, 12)[0]
    assert icons[ordinal] == ico[start:start + length], "Icon pixels missing from EXE"
    icon_sizes.append(entry[0] or 256)
assert icon_sizes == [16, 20, 24, 32, 40, 48, 64, 128, 256]
assert resource_images(16), "Missing executable version information"
print(json.dumps({"path": str(path.resolve()), "architecture": "x64", "subsystem": "Windows GUI",
                  "size_bytes": len(data), "icon_sizes": icon_sizes,
                  "imports": sorted(imports)}, indent=2))
