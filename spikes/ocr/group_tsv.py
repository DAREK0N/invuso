"""Turns ML Kit line boxes (TSV) into visual lines with the same rule as src/main.rs."""
import sys, os, glob

def group(frags):
    frags.sort(key=lambda f: f[1] + f[3])
    lines = []
    for f in frags:
        fc, fh = (f[1] + f[3]) / 2, f[3] - f[1]
        if lines:
            l = lines[-1]
            ly0 = sum(g[1] for g in l) / len(l); ly1 = sum(g[3] for g in l) / len(l)
            if abs(fc - (ly0 + ly1) / 2) < 0.5 * min(fh, ly1 - ly0):
                l.append(f); continue
        lines.append([f])
    return ["  ".join(g[4].strip() for g in sorted(l, key=lambda g: g[0])) for l in lines]

for d in sys.argv[1:]:
    for p in glob.glob(os.path.join(d, "*.tsv")):
        frags = []
        for row in open(p, encoding="utf-8").read().splitlines():
            parts = row.split("\t", 4)
            if len(parts) == 5:
                frags.append((*map(float, parts[:4]), parts[4]))
        open(p[:-4] + ".txt", "w", encoding="utf-8").write("\n".join(group(frags)) + "\n")
