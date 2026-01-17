"""Render the shipped icon group through Win32 LoadImageW/DrawIconEx, without running the app."""
import ctypes as C
from ctypes import wintypes as W
from pathlib import Path
import struct
import sys
from PIL import Image, ImageDraw

exe, output = map(Path, sys.argv[1:])
k = C.WinDLL('kernel32', use_last_error=True)
u = C.WinDLL('user32', use_last_error=True)
g = C.WinDLL('gdi32', use_last_error=True)
def api(dll, name, args, result):
    fn = getattr(dll, name)
    fn.argtypes, fn.restype = args, result
    return fn
load = api(k, 'LoadLibraryExW', [W.LPCWSTR, W.HANDLE, W.DWORD], W.HMODULE)
free = api(k, 'FreeLibrary', [W.HMODULE], W.BOOL)
load_icon = api(u, 'LoadImageW', [W.HINSTANCE, C.c_void_p, W.UINT, C.c_int, C.c_int, W.UINT], W.HANDLE)
destroy = api(u, 'DestroyIcon', [W.HANDLE], W.BOOL)
draw = api(u, 'DrawIconEx', [W.HDC, C.c_int, C.c_int, W.HANDLE, C.c_int, C.c_int, W.UINT, W.HBRUSH, W.UINT], W.BOOL)
dc = api(g, 'CreateCompatibleDC', [W.HDC], W.HDC)(None)
select = api(g, 'SelectObject', [W.HDC, W.HANDLE], W.HANDLE)
delete = api(g, 'DeleteObject', [W.HANDLE], W.BOOL)
delete_dc = api(g, 'DeleteDC', [W.HDC], W.BOOL)
create = api(g, 'CreateDIBSection', [W.HDC, C.c_void_p, W.UINT, C.POINTER(C.c_void_p), W.HANDLE, W.DWORD], W.HBITMAP)
module = load(str(exe.resolve(strict=True)), None, 0x22)
if not module or not dc:
    raise C.WinError(C.get_last_error())
sizes = (16, 20, 24, 32, 40, 48, 64, 128, 256)
width, row_height = sum(sizes) + 12 * (len(sizes) + 1), 288
height = row_height * 2
bits = C.c_void_p()
header = C.create_string_buffer(struct.pack('<IiiHHIIiiII', 40, width, -height, 1, 32, 0, width*height*4, 0, 0, 0, 0))
bitmap = create(dc, header, 0, C.byref(bits), None, 0)
if not bitmap:
    free(module); delete_dc(dc)
    raise C.WinError(C.get_last_error())
old = select(dc, bitmap)
try:
    canvas = Image.new('RGBA', (width, height), '#f6f6f6')
    ImageDraw.Draw(canvas).rectangle((0, row_height, width, height), fill='#171717')
    C.memmove(bits, canvas.tobytes('raw', 'BGRA'), width*height*4)
    for row in range(2):
        x = 12
        for size in sizes:
            icon = load_icon(module, 1, 1, size, size, 0)
            if not icon:
                raise C.WinError(C.get_last_error())
            try:
                if not draw(dc, x, row*row_height + 20, icon, size, size, 0, None, 3):
                    raise C.WinError(C.get_last_error())
            finally:
                destroy(icon)
            x += size + 12
    result = Image.frombytes('RGB', (width, height), C.string_at(bits, width*height*4), 'raw', 'BGRX')
    labels = ImageDraw.Draw(result)
    for row in range(2):
        x = 12
        for size in sizes:
            labels.text((x, row*row_height+4), str(size), fill='#222222' if row == 0 else '#eeeeee')
            x += size+12
    output.parent.mkdir(parents=True, exist_ok=True)
    result.save(output)
    print(f'Native packaged icon resource proof: {output.resolve()} (not an OS tray screenshot)')
finally:
    select(dc, old)
    delete(bitmap)
    delete_dc(dc)
    free(module)
