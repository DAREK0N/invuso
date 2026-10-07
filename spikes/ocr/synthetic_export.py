"""Exports albertobarnabo/synthetic-receipts-ocr (Apache-2.0; synthetic
thermal receipts for DE, UK, US, IT and FR) for the parser evaluation
(`crates/invuso-core/tests/jawildtext.rs`, same layout as the jawildtext
export).

For every receipt it writes into datasets/synthetic/export/<id>/:
  image.jpg      the "photographed" receipt (perspective, light, noise)
  image_clean.png the clean rendering
  boxes.tsv      word boxes of the clean rendering: left, top, right,
                 bottom, text - i.e. a perfect text recognition
  truth.tsv      currency, total, tax (minor units), date (YYYY-MM-DD),
                 time (HH:MM), store, and one `item` row per line item
                 (name, total in minor units, quantity)

Usage: python synthetic_export.py [parquet files ...]
       (default: every datasets/synthetic/*.parquet)
       SYNTHETIC_LOCALES=DE,UK,US limits the locales (default: all).
"""

import glob
import json
import os
import re
import sys
from decimal import Decimal

import pyarrow.parquet as pq

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "datasets", "synthetic")
LOCALES = [l for l in os.environ.get("SYNTHETIC_LOCALES", "").split(",") if l]


def clean(text):
    return " ".join((text or "").split())


def minor(amount):
    """Amounts of these locales all have two minor digits."""
    if amount is None:
        return ""
    return str(int((Decimal(str(amount)) * 100).to_integral_value()))


def iso_date(text, locale):
    match = re.search(r"(\d{1,4})[./-](\d{1,2})[./-](\d{2,4})", text or "")
    if not match:
        return ""
    a, b, c = match.groups()
    if len(a) == 4:
        year, month, day = a, b, c
    elif locale == "US":
        month, day, year = a, b, c
    else:
        day, month, year = a, b, c
    if len(year) == 2:
        year = "20" + year
    return f"{year}-{int(month):02}-{int(day):02}"


def hh_mm(text):
    match = re.search(r"(\d{1,2}):(\d{2})", text or "")
    return f"{int(match.group(1)):02}:{match.group(2)}" if match else ""


def export(path):
    columns = ["id", "image_clean", "image_photo", "words", "fields", "locale"]
    count = 0
    for record in pq.read_table(path, columns=columns).to_pylist():
        if LOCALES and record["locale"] not in LOCALES:
            continue
        out = os.path.join(DATA, "export", record["id"])
        os.makedirs(out, exist_ok=True)
        with open(os.path.join(out, "image.jpg"), "wb") as f:
            f.write(record["image_photo"]["bytes"])
        with open(os.path.join(out, "image_clean.png"), "wb") as f:
            f.write(record["image_clean"]["bytes"])

        rows = []
        for word in json.loads(record["words"]):
            text = clean(word["text"])
            if text:
                rows.append(
                    f"{int(word['x0'])}\t{int(word['y0'])}\t{int(word['x1']) + 1}\t{int(word['y1']) + 1}\t{text}"
                )
        with open(os.path.join(out, "boxes.tsv"), "w", encoding="utf-8", newline="\n") as f:
            f.write("\n".join(rows) + "\n")

        fields = json.loads(record["fields"])
        tax = sum(Decimal(str(t["amount"])) for t in fields.get("taxes") or [])
        lines = [
            f"currency\t{fields.get('currency') or ''}",
            f"total\t{minor(fields.get('total'))}",
            f"tax\t{minor(tax) if fields.get('taxes') else ''}",
            f"date\t{iso_date(fields.get('date'), record['locale'])}",
            f"time\t{hh_mm(fields.get('time'))}",
            f"store\t{clean(fields.get('merchant'))}",
        ]
        for line in fields.get("lines") or []:
            lines.append(f"item\t{clean(line.get('name'))}\t{minor(line.get('total'))}\t{line.get('qty') or ''}")
        with open(os.path.join(out, "truth.tsv"), "w", encoding="utf-8", newline="\n") as f:
            f.write("\n".join(lines) + "\n")
        count += 1
    return count


if __name__ == "__main__":
    paths = sys.argv[1:] or sorted(glob.glob(os.path.join(DATA, "*.parquet")))
    total = sum(export(p) for p in paths)
    print(f"exported {total} receipts to {os.path.join(DATA, 'export')}")
