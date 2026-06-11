"""Summarize a dhat-heap.json profile: top allocation sites by bytes-at-peak
(t-gmax) and bytes-at-end (t-end), plus total allocation churn.

Usage: python analyze_dhat.py path/to/dhat-heap.json
"""

import json
import sys
from collections import defaultdict


def human(n):
    for unit in ("B", "KiB", "MiB", "GiB"):
        if abs(n) < 1024.0:
            return f"{n:,.1f} {unit}"
        n /= 1024.0
    return f"{n:,.1f} TiB"


def short_frame(s):
    # ftbl entries look like "0x.... : namespace::func (file:line)"
    # strip the leading address for readability
    if " : " in s:
        s = s.split(" : ", 1)[1]
    return s


def main(path):
    with open(path, "r", encoding="utf-8") as f:
        d = json.load(f)

    ftbl = d["ftbl"]
    pps = d["pps"]

    # Field meanings (dhat v2):
    #   tb/tbk  total bytes/blocks ever allocated at this pp (churn)
    #   gb/gbk  bytes/blocks alive at the global peak (t-gmax)
    #   eb/ebk  bytes/blocks still alive at program end (t-end)
    total_end = sum(pp.get("eb", 0) for pp in pps)
    total_gmax = sum(pp.get("gb", 0) for pp in pps)
    total_churn = sum(pp.get("tb", 0) for pp in pps)

    print(f"file: {path}")
    print(f"cmd:  {d.get('cmd')}")
    print(f"pps (allocation sites): {len(pps):,}")
    print(f"total churn (ever allocated):  {human(total_churn)}")
    print(f"total alive at peak (t-gmax):  {human(total_gmax)}")
    print(f"total alive at end  (t-end):   {human(total_end)}")
    print()

    def frames_str(pp, n=6):
        out = []
        for fi in pp.get("fs", [])[:n]:
            out.append("    " + short_frame(ftbl[fi]))
        return "\n".join(out)

    for key, label in (("gb", "PEAK (t-gmax) bytes alive"), ("eb", "END (t-end) bytes alive")):
        print("=" * 78)
        print(f"TOP 25 allocation sites by {label}")
        print("=" * 78)
        ranked = sorted(pps, key=lambda pp: pp.get(key, 0), reverse=True)[:25]
        for i, pp in enumerate(ranked, 1):
            b = pp.get(key, 0)
            blocks = pp.get("gbk" if key == "gb" else "ebk", 0)
            print(f"\n#{i}  {human(b)}  in {blocks:,} blocks")
            print(frames_str(pp))
        print()

    # Aggregate by leaf+second frame to group sites that share a call origin.
    agg = defaultdict(lambda: [0, 0, 0])  # key -> [end, gmax, churn]
    for pp in pps:
        fs = pp.get("fs", [])
        key = " <- ".join(short_frame(ftbl[fi]) for fi in fs[:2]) if fs else "<no frames>"
        agg[key][0] += pp.get("eb", 0)
        agg[key][1] += pp.get("gb", 0)
        agg[key][2] += pp.get("tb", 0)

    print("=" * 78)
    print("TOP 30 by END bytes, grouped by 2 innermost frames")
    print("=" * 78)
    for key, (eb, gb, tb) in sorted(agg.items(), key=lambda kv: kv[1][0], reverse=True)[:30]:
        print(f"\nEND {human(eb):>12}  PEAK {human(gb):>12}  CHURN {human(tb):>12}")
        print("    " + key)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "dhat-heap.json")
