"""Collect license notices, normalizing line endings and trailing whitespace."""
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--offline", "--locked", "--format-version", "1",
    "--filter-platform", "x86_64-pc-windows-msvc",
], cwd=root))
parts = ["# Third-party notices\n\nGenerated from Cargo.lock and local registry license files. "
         "Includes build-time dependencies as well as runtime dependencies. "
         "The application source is MIT-licensed; dependencies retain their own licenses.\n"]
missing = []
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    if package["name"] == "print-helper":
        continue
    base = Path(package["manifest_path"]).parent
    parts.append(f"\n## {package['name']} {package['version']}\n\n"
                 f"Declared license: {package.get('license') or 'see license file'}.\n\n"
                 f"Upstream: {package.get('repository') or package.get('homepage') or 'crates.io'}.\n\n"
                 f"Authors: {'; '.join(package.get('authors', []))}.\n")
    files = sorted(p for p in base.rglob("*") if p.is_file()
                   and (any(word in p.name.lower() for word in
                           ("license", "licence", "copying", "notice", "copyright", "unlicense"))
                        or p.name in ("OFL.txt", "UFL.txt", "Hack-Regular.txt"))
                   and p.suffix.lower() not in (".rs", ".toml", ".json", ".png"))
    if package["name"] == "rust-auto-updator":
        files.append(base.parents[1] / "LICENSE")
    if not files:
        declared = package.get("license", "")
        choice = next((s for s in ("Apache-2.0", "BSL-1.0", "CC0-1.0") if s in declared), None)
        if choice:
            standard = root / "docs" / "licenses" / f"{choice}.txt"
            files = [standard]
            parts.append(f"\nUpstream crate does not package a license text. Selected {choice} "
                         "from its declared SPDX terms; standard license text follows.\n")
        else:
            missing.append(package["name"])
    for path in files:
        label = path.relative_to(base).as_posix() if path.is_relative_to(base) else path.name
        parts.append(f"\n### {label}\n\n```text\n"
                     f"{path.read_text(encoding='utf-8', errors='replace').strip()}\n```\n")
text = "\n".join(line.rstrip() for line in "\n".join(parts).splitlines()) + "\n"
(root / "THIRD_PARTY_NOTICES.md").write_text(text, encoding="utf-8")
print(f"Generated notices for {len(metadata['packages']) - 1} dependency packages.")
if missing:
    raise SystemExit("Missing upstream license files: " + ", ".join(missing))
