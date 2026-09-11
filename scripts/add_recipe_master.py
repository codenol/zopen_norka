#!/usr/bin/env python3
"""Add a composed recipe to the kit library as a master.

A recipe is a component, not a document to paste: the screen it composes
belongs in the same library as every other master, so it shows up in the
rail's Components list for any file and can be instantiated like the rest.

    python3 scripts/add_recipe_master.py            # write
    python3 scripts/add_recipe_master.py --check    # report only
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / 'design/skala-spectrum.lib.op'
PAGE_NAME = 'Recipes'

# (recipe document, master id, master name)
RECIPES = [
    (
        ROOT / 'design/recipes/ops-servers-screen.op',
        'tpl-recipe-ops-servers',
        'Recipe/Ops servers screen',
    ),
]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='report without writing')
    args = parser.parse_args()

    library = json.loads(LIB.read_text())
    pages = library.setdefault('pages', [])
    page = next((p for p in pages if p.get('name') == PAGE_NAME), None)
    if page is None:
        page = {'id': 'page-recipes', 'name': PAGE_NAME, 'children': []}
        pages.append(page)
        print(f'created page {PAGE_NAME!r}')

    existing = {child.get('id') for child in page.get('children') or []}
    added = []
    for source_path, master_id, master_name in RECIPES:
        if master_id in existing:
            continue
        document = json.loads(source_path.read_text())
        boards = document.get('children') or []
        if not boards:
            print(f'skip {master_id}: {source_path.name} carries no boards')
            continue
        master = boards[0]
        master = json.loads(json.dumps(master))
        master['id'] = master_id
        master['name'] = master_name
        master['reusable'] = True
        page.setdefault('children', []).append(master)
        added.append(master_id)
        print(f'added master {master_id} ({master_name})')

    if not added:
        print('library already carries every recipe master')
    if args.check:
        return 0
    if added:
        LIB.write_text(json.dumps(library, ensure_ascii=False, indent=1))
        print(f'wrote {LIB.relative_to(ROOT)} ({LIB.stat().st_size} bytes)')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
