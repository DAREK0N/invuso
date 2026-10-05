"""Generates THIRD-PARTY-NOTICES.md from `cargo metadata` (AGENTS.md 7.8).

Lists every crate that is compiled into the Android app (normal
dependencies, no build scripts or proc macros), the bundled non-crate
components, and the license texts that apply.

Run from the repository root after changing dependencies:

    python scripts/third-party/generate.py
"""

import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
TARGET = "aarch64-linux-android"
OUT = ROOT / "THIRD-PARTY-NOTICES.md"

# For "A OR B" the first license in this order is used.
PREFERRED = [
    "MIT", "Apache-2.0", "BSD-3-Clause", "BSD-2-Clause", "ISC", "Zlib",
    "Unicode-3.0", "MPL-2.0", "CDLA-Permissive-2.0", "BSL-1.0", "Unlicense",
    "0BSD", "MIT-0", "CC0-1.0",
]

# Where the text of each license is taken from: (crate, file, first line
# of the license proper; everything above it is that crate's copyright).
LICENSE_SOURCES = {
    "MIT": ("serde", "LICENSE-MIT", "Permission is hereby granted"),
    "Apache-2.0": ("serde", "LICENSE-APACHE", None),
    "BSD-3-Clause": ("subtle", "LICENSE", "Redistribution and use"),
    "BSD-2-Clause": ("rav1e", "LICENSE", "Redistribution and use"),
    "ISC": ("untrusted", "LICENSE.txt", "Permission to use"),
    "Zlib": ("foldhash", "LICENSE", "This software is provided"),
    "Unicode-3.0": ("icu_collections", "LICENSE", None),
    "MPL-2.0": ("cssparser", "LICENSE", None),
    "CDLA-Permissive-2.0": ("webpki-roots", "LICENSE", None),
}

LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|COPYRIGHT|NOTICE)", re.I)
# Case-sensitive on purpose: license bodies say "copyright license",
# "COPYRIGHT HOLDERS" and the like, which are not notices.
COPYRIGHT = re.compile(r"^\s*(?:[*#/;]+\s*)?((?:Copyright|COPYRIGHT)\b.*)$")


def metadata():
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", TARGET],
        cwd=ROOT, capture_output=True, check=True,
    ).stdout
    return json.loads(out.decode("utf-8"))


def shipped_packages(meta):
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = next(i for i, p in packages.items() if p["name"] == "invuso-app")
    seen, stack = set(), [root]
    while stack:
        current = stack.pop()
        if current in seen:
            continue
        seen.add(current)
        for dep in nodes[current]["deps"]:
            if not any(k["kind"] is None for k in dep["dep_kinds"]):
                continue
            package = packages[dep["pkg"]]
            if any("proc-macro" in t["kind"] for t in package["targets"]):
                continue
            stack.append(dep["pkg"])
    shipped = [packages[i] for i in seen if packages[i]["source"] is not None]
    return sorted(shipped, key=lambda p: (p["name"].lower(), p["version"]))


def elected(expression):
    """Licenses that apply, picking one alternative of every OR."""
    expression = expression.replace("/", " OR ")
    expression = re.sub(r"\s+WITH\s+\S+", "", expression)
    result = []
    for part in re.split(r"\s+AND\s+", expression.replace("(", "").replace(")", "")):
        options = [o.strip() for o in re.split(r"\s+OR\s+", part)]
        result.append(min(options, key=lambda o: PREFERRED.index(o) if o in PREFERRED else 99))
    return result


def copyright_lines(directory, authors, repository):
    lines = []
    for path in sorted(Path(directory).iterdir()):
        if not (path.is_file() and LICENSE_FILE.match(path.name)):
            continue
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            match = COPYRIGHT.match(line)
            if not match:
                continue
            text = match.group(1).strip()
            lowered = text.lower()
            if ("[yyyy]" in text or "{" in text or "copyright notice" in lowered
                    or "holder" in lowered or "owner" in lowered):
                continue
            if text not in lines:
                lines.append(text)
    if not lines and authors:
        lines = ["Copyright (c) " + ", ".join(re.sub(r"\s*<[^>]*>", "", a) for a in authors)]
    if not lines and repository:
        lines = [f"Copyright (c) the contributors of {repository}"]
    return lines[:3]


def license_text(packages, license_id):
    crate, file, start = LICENSE_SOURCES[license_id]
    package = next(p for p in packages if p["name"] == crate)
    text = (Path(package["manifest_path"]).parent / file).read_text(encoding="utf-8")
    if start:
        text = text[text.index(start):]
    return text.strip()


def sqlite_notice(packages):
    package = next(p for p in packages if p["name"] == "libsqlite3-sys")
    source = Path(package["manifest_path"]).parent / "sqlite3" / "sqlite3.c"
    head = source.read_text(encoding="utf-8", errors="replace")[:4000]
    version = re.search(r"SQLite\s+\*\* version (\S+?)\.\s", head) or re.search(r"version (\d+\.\d+\.\d+)", head)
    return version.group(1)


def cell(text):
    return text.replace("|", "\\|")


def main():
    meta = metadata()
    packages = shipped_packages(meta)
    all_packages = meta["packages"]
    ring = next(p for p in packages if p["name"] == "ring")
    ring_dir = Path(ring["manifest_path"]).parent
    tailwind_version = re.search(
        r"tailwindcss v(\S+)",
        (ROOT / "crates/invuso-app/assets/tailwind.css").read_text(encoding="utf-8"),
    ).group(1)

    rows, used = [], set()
    for package in packages:
        licenses = elected(package["license"] or "")
        used.update(licenses)
        directory = Path(package["manifest_path"]).parent
        rows.append((
            package["name"], package["version"], package["license"] or "–",
            "; ".join(copyright_lines(directory, package["authors"], package.get("repository"))) or "–",
        ))

    out = []
    out.append("# Third-Party-Notices\n")
    out.append(
        "Invuso enthält Software und Daten Dritter. Diese Datei listet alle Komponenten, "
        "die in die Android-App gebaut werden, mit Lizenz und Urheberrechtsvermerk, und "
        "danach die vollständigen Lizenztexte. Bei Crates mit mehreren Lizenzen zur Wahl "
        "(„MIT OR Apache-2.0“) gilt die zuerst passende aus MIT, Apache-2.0, BSD, ISC, Zlib.\n"
    )
    out.append("Erzeugt mit `python scripts/third-party/generate.py`; nicht von Hand bearbeiten.\n")

    out.append("## Gebündelte Komponenten\n")
    out.append("| Komponente | Version | Lizenz | Copyright |")
    out.append("|---|---|---|---|")
    out.append(f"| SQLite (über libsqlite3-sys) | {sqlite_notice(packages)} | Public Domain | – |")
    out.append("| Lucide Icons (über dioxus-free-icons) | 0.265.0 | ISC; Feather-Anteile MIT | "
               "Lucide Contributors 2022; Cole Bemis 2013-2022 |")
    out.append(f"| Tailwind CSS (erzeugtes Stylesheet) | {tailwind_version} | MIT | Tailwind Labs, Inc. |")
    out.append("| Mozilla-Root-Zertifikate (über webpki-roots) | – | CDLA-Permissive-2.0 | – |")
    out.append("| PaddleOCR PP-OCRv6 small (Texterkennungsmodelle und Wörterbuch, ONNX über RapidOCR) | PP-OCRv6 | Apache-2.0 | PaddlePaddle Authors |")
    out.append("")

    out.append("## Rust-Crates\n")
    out.append("| Crate | Version | Lizenz | Copyright |")
    out.append("|---|---|---|---|")
    for row in rows:
        out.append("| " + " | ".join(cell(c) for c in row) + " |")
    out.append("")

    out.append("## Lizenztexte\n")
    texts = [(lid, license_text(all_packages, lid)) for lid in PREFERRED if lid in used and lid in LICENSE_SOURCES]
    missing = sorted(used - set(LICENSE_SOURCES))
    if missing:
        raise SystemExit(f"no license text source for: {missing}")
    texts.append(("ring", "\n\n".join(
        (ring_dir / name).read_text(encoding="utf-8").strip()
        for name in ["LICENSE", "LICENSE-other-bits", "LICENSE-BoringSSL"]
    )))
    texts.append(("Lucide Icons", (HERE / "lucide-LICENSE.txt").read_text(encoding="utf-8").strip()))
    texts.append(("Tailwind CSS", (HERE / "tailwindcss-LICENSE.txt").read_text(encoding="utf-8").strip()))
    texts.append(("SQLite", "The author disclaims copyright to this source code. In place of\n"
                            "a legal notice, here is a blessing:\n\n"
                            "   May you do good and not evil.\n"
                            "   May you find forgiveness for yourself and forgive others.\n"
                            "   May you share freely, never taking more than you give."))
    for title, text in texts:
        out.append(f"### {title}\n")
        out.append("```text")
        out.append(text.replace("```", "'''"))
        out.append("```\n")

    OUT.write_text("\n".join(out), encoding="utf-8", newline="\n")
    print(f"{len(rows)} crates, {len(texts)} license texts -> {OUT.name}")


if __name__ == "__main__":
    main()
