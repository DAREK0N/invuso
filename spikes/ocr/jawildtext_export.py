"""Exports the receipt_kie subset of llm-jp/jawildtext (Apache-2.0) for the
parser evaluation (`crates/invuso-core/tests/jawildtext.rs`).

For every receipt it writes into datasets/jawildtext/export/<image_id>/:
  image.jpg      the photo (for the OCR run of the app)
  boxes.tsv      the annotated text regions as axis-aligned boxes:
                 left, top, right, bottom, text (same format as the
                 parser fixtures) - i.e. a perfect text recognition
  truth.json     store, date, time, total, tax and line items
  truth.tsv      the same, normalized for the Rust test: `total`, `tax`
                 (yen), `date` (YYYY-MM-DD), `time` (HH:MM), `store` and
                 one `item` row per line item (name, price, quantity)

Usage: python jawildtext_export.py [parquet files ...]
       (default: every datasets/jawildtext/*.parquet)
       JAWILDTEXT_OUT=holdout-export writes into another folder.
"""

import glob
import json
import math
import os
import re
import sys

import pyarrow.parquet as pq

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "datasets", "jawildtext")
OUT = os.environ.get("JAWILDTEXT_OUT", "export")


def value(field):
    return (field or {}).get("value")


def clean(text):
    return " ".join((text or "").split())


def yen(text, add_up=False):
    """`¥1,130`, `1130円`, `-60`, `△60` as whole yen; None without digits."""
    if not text:
        return None
    negative = any(mark in text for mark in "-−▲△")
    # Several tax amounts (`税 177 / 税 135`) add up; `,` and `.` group
    # thousands (yen has no minor unit).
    groups = [int(re.sub(r"[,.]", "", g)) for g in re.findall(r"\d[\d,.]*\d|\d", text)]
    if not groups:
        return None
    amount = sum(groups) if add_up else groups[0]
    return -amount if negative else amount


def iso_date(text):
    match = re.search(r"(20\d\d)\s*[年/.\-]\s*(\d{1,2})\s*[月/.\-]\s*(\d{1,2})", text or "")
    if not match:
        return ""
    year, month, day = match.groups()
    return f"{year}-{int(month):02}-{int(day):02}"


def hh_mm(text):
    match = re.search(r"(\d{1,2})\s*[:：時]\s*(\d{2})", text or "")
    return f"{int(match.group(1)):02}:{match.group(2)}" if match else ""


def skew(polygons):
    """Median slope (radians) of the long edges of wide text regions."""
    angles = []
    for _, points in polygons:
        edges = [(points[i], points[(i + 1) % 4]) for i in range(4)]
        (x0, y0), (x1, y1) = max(edges, key=lambda e: abs(e[1][0] - e[0][0]))
        if abs(x1 - x0) < 3 * abs(y1 - y0) or abs(x1 - x0) < 40:
            continue
        if x1 < x0:
            x0, y0, x1, y1 = x1, y1, x0, y0
        angles.append(math.atan2(y1 - y0, x1 - x0))
    if not angles:
        return 0.0
    angles.sort()
    return angles[len(angles) // 2]


def export(path):
    table = pq.read_table(path)
    count = 0
    for record in table.to_pylist():
        out = os.path.join(DATA, OUT, record["image_id"].zfill(4))
        os.makedirs(out, exist_ok=True)
        with open(os.path.join(out, "image.jpg"), "wb") as f:
            f.write(record["image"]["bytes"])

        # The app levels a photo before the parser sees it (skew_degrees of
        # the recognition); do the same with the annotated polygons.
        polygons = [
            (clean(p["text"]), p["polygon"] or [])
            for p in record["polygons"] or []
            if clean(p["text"]) and len(p["polygon"] or []) == 4
        ]
        slope = skew(polygons)
        cx = sum(x for _, pts in polygons for x, _ in pts) / max(1, 4 * len(polygons))
        cy = sum(y for _, pts in polygons for _, y in pts) / max(1, 4 * len(polygons))
        cos, sin = math.cos(-slope), math.sin(-slope)

        def level(point):
            x, y = point[0] - cx, point[1] - cy
            return (cx + x * cos - y * sin, cy + x * sin + y * cos)

        rows = []
        for text, points in polygons:
            points = [level(p) for p in points]
            xs = [p[0] for p in points]
            ys = [p[1] for p in points]
            rows.append(
                f"{int(min(xs))}\t{int(min(ys))}\t{int(max(xs)) + 1}\t{int(max(ys)) + 1}\t{text}"
            )
        with open(os.path.join(out, "boxes.tsv"), "w", encoding="utf-8", newline="\n") as f:
            f.write("\n".join(rows) + "\n")

        fields = record["fields"] or {}
        truth = {
            "store_name": value(fields.get("store_name")),
            "date": value(fields.get("date")),
            "time": value(fields.get("time")),
            "total": value(fields.get("total_amount")),
            "tax": value(fields.get("tax_amount")),
            "items": [
                {
                    "name": value(item.get("item_name")),
                    "price": value(item.get("item_price")),
                    "quantity": value(item.get("item_quantity")),
                }
                for item in fields.get("line_items") or []
            ],
        }
        with open(os.path.join(out, "truth.json"), "w", encoding="utf-8", newline="\n") as f:
            json.dump(truth, f, ensure_ascii=False, indent=1)

        def amount(text, add_up=False):
            number = yen(text, add_up)
            return "" if number is None else str(number)

        lines = [
            f"total\t{amount(truth['total'])}",
            f"tax\t{amount(truth['tax'], add_up=True)}",
            f"date\t{iso_date(truth['date'])}",
            f"time\t{hh_mm(truth['time'])}",
            f"store\t{clean(truth['store_name'])}",
        ]
        for item in truth["items"]:
            price = yen(item["price"])
            if price is not None:
                lines.append(f"item\t{clean(item['name'])}\t{price}\t{clean(item['quantity'])}")
        with open(os.path.join(out, "truth.tsv"), "w", encoding="utf-8", newline="\n") as f:
            f.write("\n".join(lines) + "\n")
        count += 1
    return count


if __name__ == "__main__":
    paths = sys.argv[1:] or sorted(glob.glob(os.path.join(DATA, "*.parquet")))
    total = sum(export(p) for p in paths)
    print(f"exported {total} receipts to {os.path.join(DATA, OUT)}")
