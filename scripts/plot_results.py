#!/usr/bin/env python3
"""Generate benchmark charts (pandas + matplotlib) for the README."""

from __future__ import annotations

import re
from pathlib import Path

import matplotlib.pyplot as plt
import pandas as pd

ROOT = Path(__file__).resolve().parents[1]
PLOTS = ROOT / "results" / "plots"
PLOTS.mkdir(parents=True, exist_ok=True)

# Muted palette, readable on light backgrounds
RAW = "#c44e52"
COMPACTED = "#4c72b0"
V2_NAIVE = "#c44e52"
V2_HIVE = "#4c72b0"
ACCENT = "#55a868"


def parse_table(md: str, section: str) -> pd.DataFrame:
    """Extract the first markdown table after a ## heading."""
    pattern = rf"#+ {re.escape(section)}[^\n]*\n+(\|[^\n]+\|\n\|[-:| ]+\|\n(?:\|[^\n]+\|\n)+)"
    match = re.search(pattern, md, re.DOTALL)
    if not match:
        raise ValueError(f"table not found for section: {section}")
    rows = []
    for line in match.group(1).strip().splitlines()[2:]:
        cells = [c.strip() for c in line.strip("|").split("|")]
        rows.append(cells)
    header = [c.strip() for c in match.group(1).splitlines()[0].strip("|").split("|")]
    return pd.DataFrame(rows, columns=header)


def save(fig: plt.Figure, name: str) -> None:
    path = PLOTS / name
    fig.savefig(path, dpi=150, bbox_inches="tight", facecolor="white")
    plt.close(fig)
    print(f"wrote {path.relative_to(ROOT)}")


def plot_v3_workloads(df: pd.DataFrame) -> None:
    df = df.copy()
    df["Raw (ms)"] = pd.to_numeric(df["Raw (ms)"])
    df["Compacted (ms)"] = pd.to_numeric(df["Compacted (ms)"])
    df["label"] = df["Workload"].str.replace(r" \([^)]+\)", "", regex=True)

    fig, ax = plt.subplots(figsize=(10, 6))
    y = range(len(df))
    h = 0.35
    ax.barh([i - h / 2 for i in y], df["Raw (ms)"], height=h, label="Raw", color=RAW)
    ax.barh([i + h / 2 for i in y], df["Compacted (ms)"], height=h, label="Compacted", color=COMPACTED)
    ax.set_yticks(list(y), df["label"])
    ax.set_xscale("log")
    ax.set_xlabel("Latency (ms, log scale)")
    ax.set_title("v3 workloads: raw vs compacted (200M rows, MinIO)")
    ax.legend(loc="lower right")
    ax.grid(axis="x", alpha=0.3)
    fig.tight_layout()
    save(fig, "v3_raw_vs_compacted.png")


def plot_concurrent(df: pd.DataFrame) -> None:
    df = df.copy()
    for col in ("p50 (ms)", "p95 (ms)", "p99 (ms)"):
        df[col] = pd.to_numeric(df[col])
    df = df.set_index("Layout")
    fig, ax = plt.subplots(figsize=(7, 4.5))
    x = range(len(df.columns))
    w = 0.35
    ax.bar([i - w / 2 for i in x], df.loc["Raw"], width=w, label="Raw", color=RAW)
    ax.bar([i + w / 2 for i in x], df.loc["Compacted"], width=w, label="Compacted", color=COMPACTED)
    ax.set_xticks(list(x), ["p50", "p95", "p99"])
    ax.set_ylabel("Latency (ms)")
    ax.set_title("Concurrent load (8 workers × 3 rounds)")
    ax.legend()
    ax.grid(axis="y", alpha=0.3)
    fig.tight_layout()
    save(fig, "v3_concurrent_latency.png")


def plot_v2_speedup() -> None:
    labels = ["Naive JSONL\n(selective 5xx)", "DataFusion Hive\n(tight partition)"]
    values = [12582.2, 19.8]
    fig, ax = plt.subplots(figsize=(6, 4))
    bars = ax.bar(labels, values, color=[V2_NAIVE, V2_HIVE], width=0.55)
    ax.set_yscale("log")
    ax.set_ylabel("Latency (ms, log scale)")
    ax.set_title("v2: partition pushdown vs naive scan (5M rows)")
    for bar, v in zip(bars, values):
        ax.text(
            bar.get_x() + bar.get_width() / 2,
            v * 1.15,
            f"{v:,.1f} ms",
            ha="center",
            va="bottom",
            fontsize=9,
        )
    ax.grid(axis="y", alpha=0.3)
    fig.tight_layout()
    save(fig, "v2_speedup.png")


def plot_compaction() -> None:
    labels = ["Raw ingest", "Compacted"]
    files = [13440, 3360]
    fig, ax = plt.subplots(figsize=(5, 4))
    bars = ax.bar(labels, files, color=[RAW, COMPACTED], width=0.5)
    ax.set_ylabel("Parquet file count")
    ax.set_title("Compaction: same rows, fewer objects")
    for bar, v in zip(bars, files):
        ax.text(bar.get_x() + bar.get_width() / 2, v + 200, f"{v:,}", ha="center", fontsize=10)
    ax.set_ylim(0, 15000)
    ax.grid(axis="y", alpha=0.3)
    fig.tight_layout()
    save(fig, "v3_compaction_files.png")


def plot_workload_classes(df: pd.DataFrame) -> None:
    """Scatter: latency vs how partition-aligned the query is (rough teaching chart)."""
    # Hand-tuned alignment score (0 = full scan, 1 = tight partition)
    classes = {
        "Tight partition (1 file)": (1.0, "partition-aligned"),
        "Dashboard (1h errors by service)": (0.85, "partition-aligned"),
        "Incident (15m 5xx window)": (0.8, "partition-aligned"),
        "Scoped day (payments 5xx)": (0.7, "partition-aligned"),
        "Full scan": (0.0, "full retention"),
        "Selective 5xx (all data)": (0.1, "full retention"),
        "Cross-day report (api routes)": (0.2, "full retention"),
        "Projection wide": (0.15, "full retention"),
        "Trace lookup": (0.05, "point lookup"),
    }
    df = df.copy()
    df["Compacted (ms)"] = pd.to_numeric(df["Compacted (ms)"])
    xs, ys, colors, labels = [], [], [], []
    color_map = {
        "partition-aligned": ACCENT,
        "full retention": RAW,
        "point lookup": "#8172b3",
    }
    for _, row in df.iterrows():
        w = row["Workload"]
        if w not in classes:
            continue
        x, kind = classes[w]
        xs.append(x)
        ys.append(row["Compacted (ms)"])
        colors.append(color_map[kind])
        labels.append(w.split(" (")[0])

    fig, ax = plt.subplots(figsize=(8, 5))
    ax.scatter(xs, ys, c=colors, s=90, zorder=3)
    ax.set_yscale("log")
    ax.set_xlabel("Partition alignment (qualitative)")
    ax.set_ylabel("Compacted latency (ms, log scale)")
    ax.set_title("Query shape drives latency more than row count alone")
    for x, y, lbl in zip(xs, ys, labels):
        ax.annotate(lbl, (x, y), textcoords="offset points", xytext=(6, 4), fontsize=8)
    ax.grid(alpha=0.3)
    fig.tight_layout()
    save(fig, "v3_workload_classes.png")


def main() -> None:
    plt.rcParams.update(
        {
            "font.family": "sans-serif",
            "font.size": 10,
            "axes.spines.top": False,
            "axes.spines.right": False,
        }
    )

    v3_md = (ROOT / "results" / "benchmarks_v3.md").read_text()
    compare = parse_table(v3_md, "Raw vs compacted")
    plot_v3_workloads(compare)

    concurrent = parse_table(v3_md, "Raw vs compacted (overall p50)")
    concurrent = concurrent.rename(columns={"Layout": "Layout", "p50 (ms)": "p50 (ms)"})
    plot_concurrent(concurrent)

    plot_v2_speedup()
    plot_compaction()
    plot_workload_classes(compare)
    print("done")


if __name__ == "__main__":
    main()
