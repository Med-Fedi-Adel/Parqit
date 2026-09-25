#!/usr/bin/env python3
"""Hand-drawn style architecture diagrams (matplotlib xkcd) for the README."""

from __future__ import annotations

from pathlib import Path

import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch, FancyArrowPatch

ROOT = Path(__file__).resolve().parents[1]
PLOTS = ROOT / "results" / "plots"
PLOTS.mkdir(parents=True, exist_ok=True)

XKCD = {"scale": 1, "length": 100, "randomness": 2}


def save(fig: plt.Figure, name: str) -> None:
    path = PLOTS / name
    fig.savefig(path, dpi=150, bbox_inches="tight", facecolor="white")
    plt.close(fig)
    print(f"wrote {path.relative_to(ROOT)}")


def box(ax, xy, w, h, label, fc="#fffef8", ec="#333", fontsize=9):
    x, y = xy
    patch = FancyBboxPatch(
        (x, y),
        w,
        h,
        boxstyle="round,pad=0.02,rounding_size=0.08",
        linewidth=1.5,
        facecolor=fc,
        edgecolor=ec,
    )
    ax.add_patch(patch)
    ax.text(x + w / 2, y + h / 2, label, ha="center", va="center", fontsize=fontsize, wrap=True)
    return (x + w / 2, y + h / 2)


def cylinder(ax, xy, w, h, label, fc="#eef6ff"):
    x, y = xy
    patch = FancyBboxPatch(
        (x, y),
        w,
        h,
        boxstyle="round,pad=0.02,rounding_size=0.15",
        linewidth=1.5,
        facecolor=fc,
        edgecolor="#333",
    )
    ax.add_patch(patch)
    ax.text(x + w / 2, y + h / 2, label, ha="center", va="center", fontsize=9)
    return (x + w / 2, y + h / 2)


def arrow(ax, p1, p2, style="-", color="#333"):
    ax.add_patch(
        FancyArrowPatch(
            p1,
            p2,
            arrowstyle="-|>",
            mutation_scale=12,
            linewidth=1.4,
            linestyle=style,
            color=color,
            shrinkA=4,
            shrinkB=4,
        )
    )


def diagram_pipeline() -> None:
    with plt.xkcd(**XKCD):
        fig, ax = plt.subplots(figsize=(11, 2.8))
        ax.set_xlim(0, 11)
        ax.set_ylim(0, 3)
        ax.axis("off")

        g = box(ax, (0.1, 1.0), 0.9, 0.7, "generator")
        raw = cylinder(ax, (1.3, 0.9), 1.1, 0.9, "raw\nParquet")
        cmp = box(ax, (2.7, 1.0), 0.85, 0.7, "compact")
        cpt = cylinder(ax, (3.8, 0.9), 1.2, 0.9, "compacted\nParquet")
        up = box(ax, (5.3, 1.0), 0.95, 0.7, "upload-v3")
        m = cylinder(ax, (6.6, 0.85), 1.0, 1.1, "MinIO")
        q = box(ax, (8.0, 1.45), 0.75, 0.6, "query")
        b = box(ax, (8.0, 0.55), 0.85, 0.6, "benchmark")
        ins = box(ax, (2.0, 2.1), 0.75, 0.55, "inspect", fc="#f5f0ff")

        arrow(ax, (g[0] + 0.35, g[1]), (raw[0] - 0.45, raw[1]))
        arrow(ax, (raw[0] + 0.45, raw[1]), (cmp[0] - 0.35, cmp[1]))
        arrow(ax, (cmp[0] + 0.35, cmp[1]), (cpt[0] - 0.45, cpt[1]))
        arrow(ax, (raw[0], raw[1] + 0.35), (up[0] - 0.2, up[1] + 0.2))
        arrow(ax, (cpt[0] + 0.45, cpt[1]), (up[0] - 0.35, up[1]))
        arrow(ax, (up[0] + 0.4, up[1]), (m[0] - 0.4, m[1]))
        arrow(ax, (m[0] + 0.4, m[1] + 0.2), (q[0] - 0.3, q[1]))
        arrow(ax, (m[0] + 0.4, m[1] - 0.2), (b[0] - 0.3, b[1]))
        arrow(ax, (ins[0], ins[1] - 0.2), (raw[0], raw[1] + 0.4), style="--")
        arrow(ax, (ins[0] + 0.2, ins[1] - 0.25), (cpt[0], cpt[1] + 0.45), style="--")

        ax.set_title("v3 pipeline", fontsize=11, loc="left")
        fig.tight_layout()
        save(fig, "diagram_pipeline.png")


def diagram_compaction() -> None:
    with plt.xkcd(**XKCD):
        fig, ax = plt.subplots(figsize=(8, 3.2))
        ax.set_xlim(0, 8)
        ax.set_ylim(0, 3.2)
        ax.axis("off")

        ax.text(1.5, 2.95, "raw: same partition", ha="center", fontsize=10, style="italic")
        files = []
        for i, name in enumerate(["batch-0000", "batch-0001", "batch-0002", "batch-0003"]):
            files.append(box(ax, (0.4, 2.1 - i * 0.55), 2.2, 0.42, name, fc="#ffeaea"))

        ax.text(6.2, 2.95, "compacted: same rows", ha="center", fontsize=10, style="italic")
        out = box(ax, (5.2, 1.35), 2.0, 0.65, "part-000.parquet", fc="#eaf2ff")

        arrow(ax, (2.7, 1.5), (5.1, 1.65))
        ax.text(3.9, 1.75, "compact", ha="center", fontsize=10)

        fig.tight_layout()
        save(fig, "diagram_compaction.png")


def diagram_pushdown() -> None:
    with plt.xkcd(**XKCD):
        fig, ax = plt.subplots(figsize=(6.5, 7))
        ax.set_xlim(0, 6.5)
        ax.set_ylim(0, 7)
        ax.axis("off")

        q = box(ax, (1.8, 6.1), 2.8, 0.55, "SQL + filters")
        pp = box(ax, (1.5, 5.0), 3.4, 0.75, "Partition keys\nin WHERE?", fc="#fff8e6")
        skip1 = box(ax, (0.2, 3.85), 2.5, 0.65, "Skip other\ndate/hour/service", fc="#eaffea")
        all1 = box(ax, (3.5, 3.85), 2.5, 0.65, "Scan all\npartitions", fc="#ffeaea")
        cp = box(ax, (1.8, 2.85), 2.8, 0.55, "Column projection")
        rg = box(ax, (1.4, 1.75), 3.6, 0.75, "Row group min/max\nrules out data?", fc="#fff8e6")
        skip2 = box(ax, (0.3, 0.55), 2.3, 0.65, "Skip row groups", fc="#eaffea")
        read = box(ax, (3.6, 0.55), 2.3, 0.65, "Read column chunks", fc="#eaf2ff")

        arrow(ax, (q[0], q[1] - 0.2), (pp[0], pp[1] + 0.3))
        arrow(ax, (pp[0] - 0.8, pp[1] - 0.3), (skip1[0], skip1[1] + 0.25))
        ax.text(1.0, 4.55, "yes", fontsize=8)
        arrow(ax, (pp[0] + 0.8, pp[1] - 0.3), (all1[0], all1[1] + 0.25))
        ax.text(5.5, 4.55, "no", fontsize=8)
        arrow(ax, (skip1[0], skip1[1] - 0.25), (cp[0] - 0.5, cp[1] + 0.2))
        arrow(ax, (all1[0], all1[1] - 0.25), (cp[0] + 0.5, cp[1] + 0.2))
        arrow(ax, (cp[0], cp[1] - 0.2), (rg[0], rg[1] + 0.3))
        arrow(ax, (rg[0] - 0.9, rg[1] - 0.3), (skip2[0], skip2[1] + 0.25))
        ax.text(1.2, 1.35, "yes", fontsize=8)
        arrow(ax, (rg[0] + 0.9, rg[1] - 0.3), (read[0], read[1] + 0.25))
        ax.text(5.3, 1.35, "no", fontsize=8)
        arrow(ax, (skip2[0] + 0.5, skip2[1] - 0.25), (read[0] - 0.5, read[1] + 0.25))

        ax.set_title("Three skips (stack in order)", fontsize=11, loc="left")
        fig.tight_layout()
        save(fig, "diagram_pushdown.png")


def diagram_workloads() -> None:
    with plt.xkcd(**XKCD):
        fig, ax = plt.subplots(figsize=(9, 3.5))
        ax.set_xlim(0, 9)
        ax.set_ylim(0, 3.5)
        ax.axis("off")

        ax.text(1.6, 3.2, "tens of ms", ha="center", fontsize=10, style="italic")
        fast = [
            box(ax, (0.3, 2.3), 2.6, 0.45, "tight partition", fc="#eaffea"),
            box(ax, (0.3, 1.75), 2.6, 0.45, "dashboard 1h", fc="#eaffea"),
            box(ax, (0.3, 1.2), 2.6, 0.45, "incident 15m", fc="#eaffea"),
        ]

        ax.text(4.0, 3.2, "seconds to minutes", ha="center", fontsize=10, style="italic")
        slow = [
            box(ax, (2.9, 2.3), 2.3, 0.45, "full scan", fc="#ffeaea"),
            box(ax, (2.9, 1.75), 2.3, 0.45, "selective 5xx", fc="#ffeaea"),
            box(ax, (2.9, 1.2), 2.3, 0.45, "cross-day report", fc="#ffeaea"),
            box(ax, (2.9, 0.65), 2.3, 0.45, "trace lookup", fc="#ffeaea"),
        ]

        p = box(ax, (5.6, 1.85), 1.5, 0.7, "all three\nskips", fc="#eaf2ff")
        l = box(ax, (5.6, 0.75), 1.5, 0.7, "list / read\nmost data", fc="#f5f0ff")

        for f in fast:
            arrow(ax, (f[0] + 1.1, f[1]), (p[0] - 0.65, p[1] + 0.1))
        ax.text(5.1, 2.5, "partition +\ncolumn +\nrow group", ha="center", fontsize=7)

        for s in slow:
            arrow(ax, (s[0] + 1.0, s[1]), (l[0] - 0.65, l[1] + 0.05))
        ax.text(5.1, 1.15, "partial\nor none", ha="center", fontsize=7)

        fig.tight_layout()
        save(fig, "diagram_workloads.png")


def main() -> None:
    diagram_pipeline()
    diagram_compaction()
    diagram_pushdown()
    diagram_workloads()
    print("done")


if __name__ == "__main__":
    main()
