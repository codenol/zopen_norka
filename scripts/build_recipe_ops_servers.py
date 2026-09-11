#!/usr/bin/env python3
"""Build the `ops-servers-screen` recipe from Skala Spectrum masters.

The pipeline is three steps, and each one exists for a reason worth keeping:

1. **compose** — `compose_genome_ops_screen.py::layout_frame` assembles the
   screen from kit masters (`ref` nodes) plus native frames for the table.
2. **expand** — the editor's web session does not render `ref` nodes from the
   kit's master library, so every reference is flattened into the master's own
   subtree with instance-scoped ids. The result is a self-contained document.
3. **tune** — the masters ship sample content (every pagination number is
   `1`, the sidebar shows the kit's own navigation, authored x/y offsets).
   This step renames the navigation, numbers the pages, clears the offsets.

Run from the repository root:

    python3 scripts/build_recipe_ops_servers.py [--push http://127.0.0.1:3100]

Without `--push` it only writes `design/recipes/ops-servers-screen.op`.
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RECIPES = ROOT / 'design/recipes'
SOURCE = RECIPES / 'ops-servers-screen.source.op'
FINAL = RECIPES / 'ops-servers-screen.op'
LIB = ROOT / 'design/skala-spectrum.lib.op'

# The product's own navigation, in order. The first entry is the current page,
# so it keeps the master's selected styling.
NAV = [
    ('Серверы', 'server'),
    ('Обзор', 'eye'),
    ('Коммутаторы', 'network'),
    ('Сети', 'share-2'),
    ('Отчёты', 'file-text'),
]


def compose() -> None:
    """Step 1 — build the screen with references intact."""
    sys.path.insert(0, str(ROOT / 'scripts'))
    import compose_genome_ops_screen as compose_mod

    document = json.loads(
        (ROOT / 'design/skala-spectrum.lib.op').read_text()
    )  # only to fail early if the library is missing
    del document
    # The composer writes through the editor API; build the document locally.
    recipe = {"pages": [{"name": "Page 1", "children": [compose_mod.layout_frame()]}]}
    SOURCE.write_text(json.dumps(recipe, ensure_ascii=False, indent=1))
    print(f'composed -> {SOURCE.name}')


def expand() -> dict:
    """Step 2 — flatten every `ref` into real nodes."""
    masters: dict[str, dict] = {}

    def index(node):
        if isinstance(node, dict):
            if isinstance(node.get('id'), str):
                masters.setdefault(node['id'], node)
            for value in node.values():
                index(value)
        elif isinstance(node, list):
            for value in node:
                index(value)

    index(json.loads(LIB.read_text()))
    expanded = 0

    def apply_overrides(nodes, overrides):
        """Patch an instance's descendants by their master-side id.

        A `children` key fills a master's slot with parts the recipe brings —
        the kit declares slots as `{"suffix": "-rows", "field": "children"}`,
        so this is the slot's own vocabulary, not a private extension.
        """
        for child in nodes:
            if not isinstance(child, dict):
                continue
            patch = overrides.get(child.get('id')) if isinstance(child.get('id'), str) else None
            if isinstance(patch, dict):
                for key, value in patch.items():
                    if key in ('descendants',):
                        continue
                    if key == 'children':
                        child['children'] = value
                    else:
                        child[key] = value
            if isinstance(child.get('children'), list):
                apply_overrides(child['children'], overrides)

    def rename_ids(node, prefix):
        """Scope every id under an instance so the flattened ids stay unique.

        A `ref` is skipped: it has not been expanded yet, and its
        `descendants` are the patches that expansion still has to apply —
        dropping them here is how the recipe's table rows lost their cells.
        """
        if isinstance(node, list):
            for child in node:
                rename_ids(child, prefix)
            return
        if not isinstance(node, dict):
            return
        if isinstance(node.get('id'), str):
            node['id'] = f'{prefix}-{node["id"]}'
        # `descendants` stay: an unexpanded ref inside this clone still needs
        # them, and an expanded one has already applied them.
        for key, value in node.items():
            if key != 'descendants':
                rename_ids(value, prefix)

    def walk(node, depth=0):
        nonlocal expanded
        if isinstance(node, list):
            return [walk(child, depth) for child in node]
        if not isinstance(node, dict):
            return node
        if node.get('type') == 'ref' and depth < 14:
            master = masters.get(node.get('ref'))
            if master is None:
                return node
            expanded += 1
            clone = json.loads(json.dumps(master))
            instance_id = str(node.get('id') or master.get('id') or 'instance')
            for key, value in node.items():
                if key not in ('type', 'ref', 'descendants', 'id'):
                    clone[key] = value
            overrides = node.get('descendants') or {}
            if overrides and isinstance(clone.get('children'), list):
                apply_overrides(clone['children'], overrides)
            clone['id'] = instance_id
            rename_ids(clone, instance_id)
            # The patches are spent; the flattened document carries none.
            clone.pop('descendants', None)
            return walk(clone, depth + 1)
        out = {key: value for key, value in node.items() if key != 'descendants'}
        if 'children' in node:
            out['children'] = walk(node['children'], depth)
        return out

    recipe = json.loads(SOURCE.read_text())
    for page in recipe['pages']:
        page['children'] = walk(page.get('children') or [])
    print(f'expanded {expanded} component references')
    return recipe


def first_of_type(node, kind):
    if isinstance(node, dict):
        if node.get('type') == kind:
            return node
        for value in node.values():
            found = first_of_type(value, kind)
            if found:
                return found
    elif isinstance(node, list):
        for value in node:
            found = first_of_type(value, kind)
            if found:
                return found
    return None


def clear_flow_offsets(node) -> int:
    """Drop authored x/y inside layout containers.

    Kit atoms carry their sample position (a table cell sits at y=108 in the
    master). Inside a horizontal/vertical parent that offset is meaningless —
    and with `clipContent` on the row it pushes the cell out of sight. A
    layout container positions its children; the offsets only survive where
    the parent is `none` (absolute) layout.
    """
    cleared = 0
    if isinstance(node, list):
        for child in node:
            cleared += clear_flow_offsets(child)
        return cleared
    if not isinstance(node, dict):
        return cleared
    children = node.get('children')
    if isinstance(children, list) and node.get('layout') in ('horizontal', 'vertical'):
        for child in children:
            if isinstance(child, dict):
                if 'x' in child:
                    child.pop('x')
                    cleared += 1
                if 'y' in child:
                    child.pop('y')
                    cleared += 1
    if isinstance(children, list):
        cleared += clear_flow_offsets(children)
    return cleared


def tune(recipe: dict) -> None:
    """Step 3 — replace the masters' sample content with this screen's."""
    counters = {'nav': 0, 'hidden': 0, 'pages': 0}

    def find_named(node, name):
        if isinstance(node, dict):
            if node.get('name') == name:
                return node
            for value in node.values():
                found = find_named(value, name)
                if found:
                    return found
        elif isinstance(node, list):
            for value in node:
                found = find_named(value, name)
                if found:
                    return found
        return None

    roots = recipe['pages'][0]['children']
    slot = find_named(roots, 'Menu slot')
    if slot is not None:
        items = [c for c in slot.get('children') or []
                 if isinstance(c, dict) and str(c.get('name', '')).startswith('MenuItem')]
        for position, item in enumerate(items):
            if position < len(NAV):
                label, icon = NAV[position]
                text = first_of_type(item, 'text')
                if text is not None:
                    text['content'] = label
                    counters['nav'] += 1
                glyph = first_of_type(item, 'icon_font')
                if glyph is not None:
                    glyph['iconFontName'] = icon
            else:
                item['visible'] = False
                counters['hidden'] += 1
        for child in slot.get('children') or []:
            if isinstance(child, dict) and str(child.get('name', '')).startswith('Heading'):
                child['visible'] = False
                counters['hidden'] += 1

    pagination = find_named(roots, 'Pagination/Default')
    if pagination is not None:
        for child in pagination.get('children') or []:
            if isinstance(child, dict):
                child.pop('x', None)
                child.pop('y', None)
        numbers = [c for c in pagination.get('children') or []
                   if isinstance(c, dict) and 'Item/Number' in str(c.get('name', ''))]
        for position, item in enumerate(numbers):
            text = first_of_type(item, 'text')
            if text is None:
                continue
            if 'Selected' in str(item.get('name')):
                text['content'] = '1'
            elif position == len(numbers) - 1:
                text['content'] = '45'
            else:
                text['content'] = str(position + 1)
            counters['pages'] += 1

    counters['offsets'] = clear_flow_offsets(recipe['pages'][0]['children'])
    print('tuned:', counters)


def push(daemon: str) -> None:
    recipe = json.loads(FINAL.read_text())

    def get(path):
        with urllib.request.urlopen(f'{daemon}{path}') as response:
            return json.load(response)

    def post(path, body):
        request = urllib.request.Request(
            f'{daemon}{path}',
            data=json.dumps(body).encode(),
            headers={'Content-Type': 'application/json'},
            method='POST',
        )
        with urllib.request.urlopen(request) as response:
            return json.load(response)

    current = get('/api/mcp/document')
    document = current['document']
    document['pages'][0]['children'] = recipe['pages'][0]['children']
    document['children'] = []
    # `preserveAuthoredGeometry` must stay false: with it on, the editor keeps
    # uncomputed layout slots and the screen renders as empty shells.
    result = post('/api/mcp/document', {
        'document': document,
        'baseVersion': current.get('version', 0),
        'activePageIndex': 0,
        'preserveAuthoredGeometry': False,
    })
    print('pushed to', daemon, ':', result.get('ok'), result.get('error', ''))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--push', metavar='URL', help='editor base URL to load the recipe into')
    args = parser.parse_args()

    compose()
    recipe = expand()
    tune(recipe)
    # Scene-template shape: boards live under a root `children`, because the
    # insert path reads the active page's children from there (and a recipe is
    # placed exactly like a scene template).
    document = {
        "version": "1.0.0",
        "name": "Ops equipment table screen",
        "children": recipe["pages"][0]["children"],
    }
    FINAL.write_text(json.dumps(document, ensure_ascii=False, indent=1))
    print(f'wrote {FINAL.relative_to(ROOT)} ({FINAL.stat().st_size} bytes)')
    if args.push:
        push(args.push.rstrip('/'))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
