#!/usr/bin/env python3
"""Add the table container/row masters to the Skala Spectrum library.

Until now the kit shipped only table *atoms* (cells, headers, kebab), so every
screen had to hand-build the container and the rows around them. This script
adds the two missing structural masters, plus a header-row master:

    organism-table-base        vertical container: rounded, bordered, clips
      organism-table-header    slot (`-header`): the header row band
      organism-table-rows      slot (`-rows`): the body rows
    organism-table-row         one 44px body row
      organism-table-row-cells slot (`-cells`): the row's cells
    organism-table-header-row  one 52px header row
      organism-table-header-row-cells  slot (`-cells`): the header cells

Slots follow the kit's existing convention (`kit.json` types describe them as
`{"suffix": "-main", "field": "children"}`), so a host that knows how to fill
`tpl-layout-main` already knows how to fill these.

Run from the repository root:

    python3 scripts/add_table_masters.py            # write
    python3 scripts/add_table_masters.py --check    # report only
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / 'design/skala-spectrum.lib.op'
PAGE_NAME = 'Organisms · Table'


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def frame(node_id: str, name: str, **props) -> dict:
    return {"type": "frame", "id": node_id, "name": name, **props}


def table_base() -> dict:
    """The table container: a bordered, rounded, clipping vertical frame."""
    return frame(
        'organism-table-base',
        'Table/Default',
        reusable=True,
        x=0.0,
        y=0.0,
        width='fill_container',
        height='fit_content',
        layout='vertical',
        gap=0.0,
        clipContent=True,
        cornerRadius=8.0,
        stroke={"thickness": 1, "fill": solid('$table/header/border/default')},
        children=[
            frame(
                'organism-table-header',
                'Header',
                width='fill_container',
                height=52.0,
                layout='horizontal',
                clipContent=True,
                fill=solid('$table/header/background/default'),
                children=[],
            ),
            frame(
                'organism-table-rows',
                'Rows',
                width='fill_container',
                height='fit_content',
                layout='vertical',
                gap=0.0,
                clipContent=True,
                children=[],
            ),
        ],
    )


def table_row() -> dict:
    """One body row: 44px, clips, and holds the cells in a horizontal slot."""
    return frame(
        'organism-table-row',
        'Table/Row/Default',
        reusable=True,
        x=0.0,
        y=0.0,
        width='fill_container',
        height=44.0,
        layout='horizontal',
        alignItems='center',
        clipContent=True,
        fill=solid('$table/row/background/default'),
        children=[
            frame(
                'organism-table-row-cells',
                'Cells',
                width='fill_container',
                height=44.0,
                layout='horizontal',
                alignItems='center',
                clipContent=True,
                children=[],
            )
        ],
    )


def table_header_row() -> dict:
    """One header row: 52px, and holds the header cells in a horizontal slot."""
    return frame(
        'organism-table-header-row',
        'Table/HeaderRow/Default',
        reusable=True,
        x=0.0,
        y=0.0,
        width='fill_container',
        height=52.0,
        layout='horizontal',
        alignItems='center',
        clipContent=True,
        fill=solid('$table/header/background/default'),
        children=[
            frame(
                'organism-table-header-row-cells',
                'Cells',
                width='fill_container',
                height=52.0,
                layout='horizontal',
                alignItems='center',
                clipContent=True,
                children=[],
            )
        ],
    )


MASTERS = [table_base, table_header_row, table_row]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='report without writing')
    args = parser.parse_args()

    library = json.loads(LIB.read_text())
    pages = library.setdefault('pages', [])
    page = next((p for p in pages if p.get('name') == PAGE_NAME), None)
    if page is None:
        page = {'id': 'page-organisms-table', 'name': PAGE_NAME, 'children': []}
        pages.append(page)
        print(f'created page {PAGE_NAME!r}')

    existing = {master.get('id') for master in page.get('children') or []}
    added = []
    for factory in MASTERS:
        master = factory()
        if master['id'] in existing:
            continue
        page.setdefault('children', []).append(master)
        added.append(master['id'])
        print(f'added master {master["id"]} ({master["name"]})')

    if not added:
        print('library already carries every table master')
    if args.check:
        return 0
    if added:
        LIB.write_text(json.dumps(library, ensure_ascii=False, indent=1))
        print(f'wrote {LIB.relative_to(ROOT)} ({LIB.stat().st_size} bytes)')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
