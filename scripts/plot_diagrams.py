#!/usr/bin/env python3
"""Sysdraw-style diagrams: Rough.js strokes, clean layout, dotted canvas."""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

from rough import Options, canvas

ROOT = Path(__file__).resolve().parents[1]
PLOTS = ROOT / "results" / "plots"
PLOTS.mkdir(parents=True, exist_ok=True)

STROKE = "#374151"
MUTED = "#6b7280"
BG = "#fafafa"

FILLS = {
    "process": "#ffffff",
    "storage": "#dbeafe",
    "decision": "#fef9c3",
    "good": "#dcfce7",
    "slow": "#fee2e2",
    "accent": "#ede9fe",
}


def opts(**kw) -> Options:
    base = dict(stroke=STROKE, strokeWidth=1.4, roughness=1.05, bowing=0.6)
    base.update(kw)
    return Options(**base)


@dataclass
class Node:
    x: float
    y: float
    w: float
    h: float
    label: str
    kind: Literal["process", "storage", "decision", "good", "slow", "accent"] = "process"

    @property
    def cx(self) -> float:
        return self.x + self.w / 2

    @property
    def cy(self) -> float:
        return self.y + self.h / 2

    def port(self, side: str) -> tuple[float, float]:
        return {
            "left": (self.x, self.cy),
            "right": (self.x + self.w, self.cy),
            "top": (self.cx, self.y),
            "bottom": (self.cx, self.y + self.h),
        }[side]


class Diagram:
    def __init__(self, width: int, height: int, title: str = "") -> None:
        self.width = width
        self.height = height
        self.title = title
        self.nodes: list[Node] = []
        self.paths: list[str] = []
        self.labels: list[str] = []
        self.tags: list[str] = []

    def _rough_rect(self, n: Node) -> str:
        pad = 12
        cw, ch = int(n.w + pad * 2), int(n.h + pad * 2)
        c = canvas(cw, ch)
        c.rectangle(pad, pad, n.w, n.h, opts(fill=FILLS[n.kind], fillStyle="solid"))
        frag = c.as_svg(cw, ch)
        inner = frag.split("<svg", 1)[1].split(">", 1)[1].rsplit("</svg>", 1)[0]
        return f'<g transform="translate({n.x - pad:.1f},{n.y - pad:.1f})">{inner}</g>'

    def _rough_line(self, x1: float, y1: float, x2: float, y2: float, *, dashed: bool = False) -> None:
        pad = 20
        min_x, max_x = min(x1, x2) - pad, max(x1, x2) + pad
        min_y, max_y = min(y1, y2) - pad, max(y1, y2) + pad
        cw, ch = max(int(max_x - min_x), 40), max(int(max_y - min_y), 40)
        c = canvas(cw, ch)
        lx1, ly1 = x1 - min_x, y1 - min_y
        lx2, ly2 = x2 - min_x, y2 - min_y
        dash = [7, 6] if dashed else None
        c.line(lx1, ly1, lx2, ly2, opts(strokeWidth=1.3, roughness=0.9, strokeLineDash=dash))
        frag = c.as_svg(cw, ch)
        inner = frag.split("<svg", 1)[1].split(">", 1)[1].rsplit("</svg>", 1)[0]
        self.paths.append(f'<g transform="translate({min_x:.1f},{min_y:.1f})">{inner}</g>')

    def _rough_polyline(self, points: list[tuple[float, float]], *, dashed: bool = False) -> None:
        if len(points) < 2:
            return
        pad = 24
        xs = [p[0] for p in points]
        ys = [p[1] for p in points]
        min_x, min_y = min(xs) - pad, min(ys) - pad
        max_x, max_y = max(xs) + pad, max(ys) + pad
        cw, ch = max(int(max_x - min_x), 40), max(int(max_y - min_y), 40)
        c = canvas(cw, ch)
        dash = [7, 6] if dashed else None
        for i in range(len(points) - 1):
            x1, y1 = points[i][0] - min_x, points[i][1] - min_y
            x2, y2 = points[i + 1][0] - min_x, points[i + 1][1] - min_y
            c.line(x1, y1, x2, y2, opts(strokeWidth=1.3, roughness=0.9, strokeLineDash=dash))
        frag = c.as_svg(cw, ch)
        inner = frag.split("<svg", 1)[1].split(">", 1)[1].rsplit("</svg>", 1)[0]
        self.paths.append(f'<g transform="translate({min_x:.1f},{min_y:.1f})">{inner}</g>')

    def add(self, node: Node) -> Node:
        self.nodes.append(node)
        self.paths.append(self._rough_rect(node))
        lines = node.label.split("\n")
        fs = 13 if len(lines) == 1 else 12
        lh = fs + 5
        y0 = node.cy - (len(lines) - 1) * lh / 2
        for i, line in enumerate(lines):
            self.labels.append(
                f'<text x="{node.cx:.1f}" y="{y0 + i * lh:.1f}" '
                f'font-family="Inter,Segoe UI,system-ui,sans-serif" font-size="{fs}" '
                f'font-weight="500" fill="{STROKE}" text-anchor="middle">{line}</text>'
            )
        return node

    def tag(self, x: float, y: float, text: str) -> None:
        self.tags.append(
            f'<text x="{x:.1f}" y="{y:.1f}" font-family="Inter,Segoe UI,system-ui,sans-serif" '
            f'font-size="12" font-weight="500" fill="{MUTED}" font-style="italic" '
            f'text-anchor="middle">{text}</text>'
        )

    def harrow(self, a: Node, b: Node, gap: float = 18) -> None:
        self._rough_line(a.port("right")[0] + gap / 2, a.cy, b.port("left")[0] - gap / 2, b.cy)

    def varrow(self, a: Node, b: Node, gap: float = 16) -> None:
        self._rough_line(a.cx, a.port("bottom")[1] + gap / 2, b.cx, b.port("top")[1] - gap / 2)

    def arrow(self, p1: tuple[float, float], p2: tuple[float, float], *, dashed: bool = False) -> None:
        self._rough_line(p1[0], p1[1], p2[0], p2[1], dashed=dashed)

    def route(
        self,
        src: Node,
        dst: Node,
        *,
        via_y: float | None = None,
        dashed: bool = False,
    ) -> None:
        x1, y1 = src.port("right")
        x2, y2 = dst.port("left")
        mid_y = via_y if via_y is not None else min(src.y, dst.y) - 36
        pts = [(x1, y1), (x1 + 24, y1), (x1 + 24, mid_y), (x2 - 24, mid_y), (x2 - 24, y2), (x2, y2)]
        self._rough_polyline(pts, dashed=dashed)

    def branch(self, src: Node, dst: Node, side: str, label: str = "") -> None:
        sx, sy = src.port("bottom")
        dx, dy = dst.port("top")
        drop = 36
        mid = sy + drop
        pts = [(sx, sy), (sx, mid), (dx, mid), (dx, dy)]
        self._rough_polyline(pts)
        if label:
            lx = sx - 18 if side == "left" else sx + 18
            self.labels.append(
                f'<text x="{lx:.1f}" y="{mid - 10:.1f}" font-family="Inter,Segoe UI,sans-serif" '
                f'font-size="11" fill="{MUTED}" text-anchor="middle">{label}</text>'
            )

    def save(self, name: str) -> None:
        svg_path = PLOTS / name.replace(".png", ".svg")
        png_path = PLOTS / name

        title = (
            f'<text x="36" y="40" font-family="Inter,Segoe UI,sans-serif" '
            f'font-size="18" font-weight="600" fill="{STROKE}">{self.title}</text>\n'
            if self.title
            else ""
        )

        svg = f"""<svg width="{self.width}" height="{self.height}" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <pattern id="dots" width="22" height="22" patternUnits="userSpaceOnUse">
      <circle cx="1.5" cy="1.5" r="1.1" fill="#d1d5db"/>
    </pattern>
  </defs>
  <rect width="100%" height="100%" fill="{BG}"/>
  <rect width="100%" height="100%" fill="url(#dots)"/>
  {title}
  {"".join(self.paths)}
  {"".join(self.labels)}
  {"".join(self.tags)}
</svg>
"""
        svg_path.write_text(svg)
        try:
            subprocess.run(
                ["rsvg-convert", "-w", str(self.width), "-o", str(png_path), str(svg_path)],
                check=True,
                capture_output=True,
            )
            print(f"wrote {png_path.relative_to(ROOT)}")
        except (FileNotFoundError, subprocess.CalledProcessError):
            print(f"wrote {svg_path.relative_to(ROOT)} (rsvg-convert missing)")


def diagram_pipeline() -> None:
    d = Diagram(1260, 300, "v3 pipeline")

    y = 152
    g = d.add(Node(36, y, 120, 58, "generator"))
    raw = d.add(Node(210, y - 10, 130, 74, "raw\nParquet", "storage"))
    cmp = d.add(Node(394, y, 108, 58, "compact"))
    cpt = d.add(Node(556, y - 10, 140, 74, "compacted\nParquet", "storage"))
    up = d.add(Node(750, y, 120, 58, "upload-v3"))
    m = d.add(Node(916, y - 12, 116, 78, "MinIO", "storage"))
    q = d.add(Node(1088, y - 58, 104, 54, "query"))
    b = d.add(Node(1088, y + 38, 114, 54, "benchmark"))
    ins = d.add(Node(400, 34, 110, 50, "inspect", "accent"))

    d.harrow(g, raw)
    d.harrow(raw, cmp)
    d.harrow(cmp, cpt)
    d.harrow(cpt, up)
    d.harrow(up, m)
    d.arrow(m.port("right"), (q.port("left")[0] - 12, q.cy))
    d.arrow(m.port("right"), (b.port("left")[0] - 12, b.cy))
    d.arrow(ins.port("bottom"), raw.port("top"), dashed=True)
    d.arrow(ins.port("bottom"), cpt.port("top"), dashed=True)

    d.save("diagram_pipeline.png")


def diagram_compaction() -> None:
    d = Diagram(760, 380, "Compaction")

    d.tag(170, 56, "raw · same partition")
    d.tag(560, 56, "compacted · same rows")

    files = [
        d.add(Node(70, 90 + i * 66, 200, 46, name, "slow"))
        for i, name in enumerate(
            ["part-batch-0000", "part-batch-0001", "part-batch-0002", "part-batch-0003"]
        )
    ]
    out = d.add(Node(460, 168, 210, 54, "part-000.parquet", "storage"))
    d.arrow((files[1].port("right")[0] + 16, files[1].cy), (out.port("left")[0] - 16, out.cy))
    d.labels.append(
        f'<text x="360" y="188" font-family="Inter,sans-serif" font-size="12" '
        f'fill="{MUTED}" text-anchor="middle">compact</text>'
    )

    d.save("diagram_compaction.png")


def diagram_pushdown() -> None:
    d = Diagram(720, 860, "Three skips")

    q = d.add(Node(230, 70, 260, 48, "SQL + filters"))
    pp = d.add(Node(210, 170, 300, 56, "Partition keys in WHERE?", "decision"))
    skip = d.add(Node(60, 310, 230, 58, "Skip other\ndate / hour / service", "good"))
    scan = d.add(Node(430, 310, 230, 58, "Scan all\npartitions", "slow"))
    cp = d.add(Node(230, 430, 260, 48, "Column projection"))
    rg = d.add(Node(210, 540, 300, 56, "Row group min/max\nrules out data?", "decision"))
    skip_rg = d.add(Node(80, 680, 210, 52, "Skip row groups", "good"))
    read = d.add(Node(430, 680, 210, 52, "Read column chunks", "storage"))

    d.varrow(q, pp, gap=20)
    d.branch(pp, skip, "left", "yes")
    d.branch(pp, scan, "right", "no")
    d.varrow(skip, cp, gap=22)
    d.varrow(scan, cp, gap=22)
    d.varrow(cp, rg, gap=20)
    d.branch(rg, skip_rg, "left", "yes")
    d.branch(rg, read, "right", "no")
    d._rough_polyline(
        [
            skip_rg.port("bottom"),
            (skip_rg.cx, read.y - 28),
            (read.cx, read.y - 28),
            read.port("top"),
        ]
    )

    d.save("diagram_pushdown.png")


def diagram_workloads() -> None:
    d = Diagram(980, 420, "Workload latency classes")

    d.tag(170, 58, "tens of ms")
    d.tag(470, 58, "seconds to minutes")

    fast = d.add(
        Node(
            60,
            90,
            220,
            120,
            "tight partition\ndashboard · 1h\nincident · 15m",
            "good",
        )
    )
    slow = d.add(
        Node(
            360,
            90,
            220,
            168,
            "full scan\nselective 5xx\ncross-day report\ntrace lookup",
            "slow",
        )
    )
    skips = d.add(Node(680, 110, 230, 72, "All three skips\npartition · column · row group", "accent"))
    heavy = d.add(Node(680, 250, 230, 64, "List / read\nmost data", "storage"))

    d.harrow(fast, skips)
    d.harrow(slow, heavy)
    d.labels.append(
        f'<text x="600" y="156" font-family="Inter,sans-serif" font-size="11" '
        f'fill="{MUTED}" text-anchor="middle">partition-aligned</text>'
    )
    d.labels.append(
        f'<text x="600" y="296" font-family="Inter,sans-serif" font-size="11" '
        f'fill="{MUTED}" text-anchor="middle">full retention / lookup</text>'
    )

    d.save("diagram_workloads.png")


def main() -> None:
    diagram_pipeline()
    diagram_compaction()
    diagram_pushdown()
    diagram_workloads()
    print("done")


if __name__ == "__main__":
    main()
