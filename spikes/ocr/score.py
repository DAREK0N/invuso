"""Scores OCR results against ground_truth.json (AP-S1 spike)."""
import json, sys, os, re, unicodedata
from difflib import SequenceMatcher

def norm(s):
    s = unicodedata.normalize("NFKC", s)
    return "".join(c for c in s if not c.isspace()).lower()

def sim(a, b):
    return SequenceMatcher(None, norm(a), norm(b)).ratio()

def tokens(line):
    line = unicodedata.normalize("NFKC", line)
    # Spaces right after a decimal/thousands separator ("9, 99") are a pure
    # formatting artefact the parser can normalize; tolerate them for all engines.
    line = re.sub(r"(\d)([,.]) (\d)", r"\1\2\3", line)
    return [t for t in line.replace("円", " ").replace("込", " ").split()]

def has_price(line, price):
    # Price must appear as its own token (allow trailing tax letter / minus suffix).
    p = norm(price)
    for t in tokens(line):
        t = norm(t).rstrip("abd-*").lstrip("¥")
        if t == p or t.endswith(p) and not t[: -len(p)][-1:].isdigit() and len(t) - len(p) <= 3:
            return True
    return False

gt = json.load(open("ground_truth.json", encoding="utf-8"))
engines = sys.argv[1:]
rows = []
for eng in engines:
    agg = {"de": [0, 0, 0, 0, 0.0, 0], "ja": [0, 0, 0, 0, 0.0, 0]}
    detail = []
    for name, g in gt.items():
        path = f"results/{eng}/{name}.txt"
        if not os.path.exists(path):
            continue
        lines = open(path, encoding="utf-8").read().splitlines()
        lang = name[:2]
        a = agg[lang]
        ok_items = 0
        for item, price in g["items"]:
            best = max(lines, key=lambda l: sim(l, item + " " + price)) if lines else ""
            name_sim = max((SequenceMatcher(None, norm(item), norm(l)).find_longest_match().size / max(len(norm(item)), 1) for l in [best]), default=0)
            # name similarity: best matching line's similarity on the name part
            ns = max((sim(item, l) if len(norm(l)) <= len(norm(item)) + 2 else sim(item, l[: len(item) + 4]) for l in lines), default=0)
            row_ok = any(has_price(l, price) and sim(item, l) >= 0.5 for l in lines) or (has_price(best, price) and ns >= 0.8)
            a[0] += 1; a[1] += row_ok; a[4] += ns; ok_items += row_ok
        if g["total"]:
            a[2] += 1
            tot_ok = any(has_price(l, g["total"]) for l in lines)
            a[3] += tot_ok
        else:
            tot_ok = None
        for t in g.get("text", []):
            a[5] += 1
            a[4] += max((sim(t, l) for l in lines), default=0)
            a[0] += 0
        detail.append(f"  {name:20} items {ok_items}/{len(g['items'])} total {tot_ok}")
    print(f"== {eng}")
    print("\n".join(detail))
    for lang, a in agg.items():
        n_names = a[0] + a[5]
        print(f"  {lang}: Positionen {a[1]}/{a[0]}  Summe {a[3]}/{a[2]}  Namens-Ähnlichkeit {a[4]/max(n_names,1):.2f}")
