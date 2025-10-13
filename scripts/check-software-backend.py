"""Enforce the active Windows UI dependency graph without executing an EXE."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
tree = subprocess.check_output([
    "cargo", "tree", "--offline", "--locked", "--target", "x86_64-pc-windows-msvc",
    "--edges", "normal,build", "--prefix", "none", "--format", "{p}",
], cwd=root, text=True)
names = {line.split()[0] for line in tree.splitlines() if line.strip()}
forbidden = {"eframe", "egui_glow", "egui-wgpu", "glow", "glutin", "glutin-winit",
             "gl_generator", "wgpu", "wgpu-core", "wgpu-hal", "ash", "vulkano"}
assert not (names & forbidden), f"GPU/UI dependency regression: {sorted(names & forbidden)}"
required = {"egui", "egui-winit", "egui_software_backend", "softbuffer", "winit"}
assert required <= names, f"Missing software backend dependencies: {sorted(required - names)}"
print("Windows dependency graph: CPU rasterizer and software presenter; no OpenGL/GPU backend.")
