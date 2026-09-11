#!/usr/bin/env python3
"""Compose the Genom servers ops screen from Skala masters and push to the daemon."""

from __future__ import annotations

import json
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DAEMON = "http://127.0.0.1:3007"

COLS = [
    ("name", "Имя", 115),
    ("serial", "Серийный номер", 125),
    ("pak", "ПАК", 100),
    ("vendor", "Производитель", 115),
    ("model", "Модель", 115),
    ("cpu", "CPU", 130),
    ("ram", "RAM", 70),
    ("disks", "Диски", 150),
    ("bmc", "BMC IP", 115),
]

ROW = {
    "name": "s3m-03b02-mak44",
    "serial": "0103230BA0",
    "pak": "S3-PRD-2m0002",
    "vendor": "Yadro",
    "model": "YADRO X3-205",
    "cpu": "2x Intel Xeon 5317",
    "ram": "2x 32Gb",
    "disks": "2x 0.48TB SSD, 1x\n0.48TB SSD, 4x\n1.92TB SSD",
    "bmc": "192.168.191.151",
}

N_ROWS = 6


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def ref(node_id: str, master: str, descendants: dict | None = None, **props) -> dict:
    node: dict = {"type": "ref", "id": node_id, "ref": master, **props}
    if descendants:
        node["descendants"] = descendants
    return node


def text_cell(node_id: str, content: str, alt: bool, width: float) -> dict:
    """One body cell — the kit's `Table/Cell/Text` atom, retitled."""
    master = (
        "atom-table-cell-text-alternative" if alt else "atom-table-cell-text-default"
    )
    # The atom's own content and rule carry a fixed 200px width; a narrow
    # column has to restate both or the text overruns its neighbours.
    return ref(
        node_id,
        master,
        {
            f"{master}-content": {
                "width": width,
                "height": 44.0,
                "padding": [0, 8.0],
                "clipContent": True,
            },
            f"{master}-rule": {"width": width},
            f"{master}-label": {"content": content, "fontSize": 13.0},
        },
        width=width,
        height=44.0,
    )


def header_cell(node_id: str, label: str, width: float) -> dict:
    """One header cell — the kit's sort/filter header atom, retitled."""
    return ref(
        node_id,
        "atom-table-header-text-sort-filter",
        {
            "atom-table-header-text-sort-filter-content": {
                "width": width,
                "height": 52.0,
                "padding": [0, 8.0],
                "clipContent": True,
            },
            "atom-table-header-text-sort-filter-rule": {"width": width},
            "atom-table-header-text-sort-filter-label": {"content": label, "fontSize": 13.0},
        },
        width=width,
        height=52.0,
    )


def kebab_cell(node_id: str, alt: bool) -> dict:
    """The row-actions cell — the kit's kebab cell."""
    master = (
        "atom-table-cell-kebab-alternative" if alt else "atom-table-cell-kebab-default"
    )
    return ref(node_id, master, width=44.0, height=44.0)


def icon_button(node_id: str, icon: str) -> dict:
    """A 32px icon-only button — the kit's own outline/secondary button."""
    master = "atom-button-outline-large-secondary-default-icon"
    return ref(
        node_id,
        master,
        {f"{master}-icon": {"iconFontName": icon}},
        width=32.0,
        height=32.0,
    )


def select_chip(node_id: str, label: str) -> dict:
    """A field with a trailing chevron — the kit's `Input/Icon/Trailing`."""
    master = "atom-input-icon-trailing-default"
    return ref(
        node_id,
        master,
        {
            f"{master}-label": {"content": label},
            f"{master}-icon": {"iconFontName": "chevron-down"},
        },
        width=120.0,
        height=32.0,
    )


def toolbar() -> dict:
    return {
        "type": "frame",
        "id": "ops-toolbar",
        "name": "Toolbar",
        "width": "fill_container",
        "height": 32.0,
        "layout": "horizontal",
        "gap": 12.0,
        "alignItems": "center",
        "children": [
            ref(
                "ops-search",
                "atom-input-icon-leading-default",
                {
                    "atom-input-icon-leading-default-label": {"content": "Поиск"},
                    "atom-input-icon-leading-default-icon": {
                        "iconFontName": "search"
                    },
                },
                width=236.0,
                height=32.0,
            ),
            select_chip("ops-pak-select", "ПАК"),
            icon_button("ops-filter", "funnel"),
            {
                "type": "frame",
                "id": "ops-toolbar-spacer",
                "name": "spacer",
                "width": "fill_container",
                "height": 1.0,
            },
            icon_button("ops-export", "download"),
        ],
    }


def table_header_row() -> dict:
    """The header band — the kit's `Table/HeaderRow` master, cells inside."""
    cells = [header_cell(f"ops-th-{key}", label, width) for key, label, width in COLS]
    cells.append(
        ref("ops-th-actions", "atom-table-header-columns", width=56.0, height=52.0)
    )
    return ref(
        "ops-header-row",
        "organism-table-header-row",
        {"organism-table-header-row-cells": {"children": cells}},
        height=52.0,
    )


def table_row(index: int) -> dict:
    """One body row — the kit's `Table/Row` master, cells inside."""
    alt = index % 2 == 1
    cells = [
        text_cell(f"ops-r{index}-{key}", ROW[key], alt, width)
        for key, _label, width in COLS
    ]
    cells.append(kebab_cell(f"ops-r{index}-kebab", alt))
    master = "organism-table-row"
    return ref(
        f"ops-row-{index}",
        master,
        {f"{master}-cells": {"children": cells}},
        height=44.0,
        **(
            {"fill": solid("$table/row/background/alternative")}
            if alt
            else {}
        ),
    )


def table_block() -> dict:
    """The table container — the kit's `Table/Default` master, slots filled."""
    return ref(
        "ops-table",
        "organism-table-base",
        {
            "organism-table-header": {"children": [table_header_row()]},
            "organism-table-rows": {"children": [table_row(i) for i in range(N_ROWS)]},
        },
        width="fill_container",
    )


def footer() -> dict:
    return {
        "type": "frame",
        "id": "ops-footer",
        "name": "Footer",
        "width": "fill_container",
        "height": 32.0,
        "layout": "horizontal",
        "justifyContent": "end",
        "alignItems": "center",
        "children": [
            ref("ops-pagination", "molecule-pagination-default"),
        ],
    }


def content_column() -> dict:
    return {
        "type": "frame",
        "id": "ops-content",
        "name": "Content",
        "x": 287.0,
        "y": 20.0,
        "width": 1137.0,
        "height": 814.0,
        "layout": "vertical",
        "gap": 16.0,
        "alignItems": "start",
        "children": [
            ref(
                "ops-breadcrumbs",
                "molecule-breadcrumbs-2",
                {
                    "molecule-breadcrumbs-2-label-0": {"content": "Геном"},
                    "molecule-breadcrumbs-2-label-1": {"content": "Коммутаторы"},
                },
            ),
            main_container(),
        ],
    }


def main_container() -> dict:
    return {
        "type": "frame",
        "id": "ops-main",
        "name": "Main container",
        "width": "fill_container",
        "height": 750.0,
        "layout": "vertical",
        "gap": 16.0,
        "padding": 24.0,
        "alignItems": "start",
        "cornerRadius": 16.0,
        "clipContent": True,
        "fill": solid("$container/background/default"),
        "stroke": {
            "thickness": 1,
            "fill": solid("$container/border/default"),
        },
        "children": [toolbar(), table_block(), footer()],
    }


def layout_frame() -> dict:
    return {
        "type": "frame",
        "id": "ops-layout",
        "name": "Layout/Default",
        "x": 0,
        "y": 0,
        "width": 1440.0,
        "height": 850.0,
        "layout": "none",
        "clipContent": True,
        "fill": solid("$layout/background/default"),
        "children": [
            ref(
                "ops-sidebar",
                "org-sidebar-default",
                x=20.0,
                y=20.0,
                width=251.0,
                height=814.0,
            ),
            content_column(),
        ],
    }


def get_json(path: str):
    with urllib.request.urlopen(f"{DAEMON}{path}") as resp:
        return json.load(resp)


def post_json(path: str, body: dict):
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        f"{DAEMON}{path}",
        data=data,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req) as resp:
        return json.load(resp)


def main() -> int:
    current = get_json("/api/mcp/document")
    version = current.get("version", 0)
    doc = current["document"]

    # Keep kit pages / variables / themes; replace only the design page content.
    pages = doc.get("pages") or []
    if not pages:
        print("no pages", file=sys.stderr)
        return 1
    pages[0]["children"] = [layout_frame()]
    pages[0]["name"] = "Page 1"
    doc["pages"] = pages
    doc["children"] = []

    result = post_json(
        "/api/mcp/document",
        {
            "document": doc,
            "baseVersion": version,
            "activePageIndex": 0,
            "preserveAuthoredGeometry": False,
        },
    )
    print(json.dumps(result, indent=2))
    after = get_json("/api/mcp/document")
    kids = after["document"]["pages"][0]["children"]
    print("design roots:", [(c.get("id"), c.get("name")) for c in kids])
    return 0 if result.get("ok") else 1


if __name__ == "__main__":
    raise SystemExit(main())
