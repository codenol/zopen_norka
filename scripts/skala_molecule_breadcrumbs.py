"""Breadcrumbs — hug-width trail in a 48px XXL shell (molecule, not atom)."""

from __future__ import annotations

# Figma SVG set is 347×304: five 47px tiles (y=0.5 → treat as 48) with
# 16px gap. Inspector 546×344 / radius 5 is Figma set chrome — ignore.
# rx 15.5 + 1px centre stroke = `$border-radius/XXL` (16). Skip blur.
# Widths hug: 82 / 145 / 212 / 279 / 346. `lots`: 1, 2, 3, 4, 4+.
BC_H = 48.0
BC_RADIUS = 16.0
BC_PAD_X = 24.0
BC_GAP = 16.0
BC_SET_GAP = 16.0
BC_DOT = 4.0
BC_FONT = 14
BC_WEIGHT = 500
BC_LINE_HEIGHT = round(18 / 14, 4)
BC_SAMPLE = "Label"
BC_FILL = "$breadcrumbs/background/default"
BC_BORDER = "$breadcrumbs/border/default"
BC_TEXT = "$breadcrumbs/text/default"
BC_SECONDARY = "$breadcrumbs/text/secondary"

LOTS = (
    {"id": "1", "label": "1", "count": 1},
    {"id": "2", "label": "2", "count": 2},
    {"id": "3", "label": "3", "count": 3},
    {"id": "4", "label": "4", "count": 4},
    {"id": "4-plus", "label": "4+", "count": 5},
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def crumb_label(lots_id: str, index: int, current: bool) -> dict:
    return {
        "type": "text",
        "id": f"molecule-breadcrumbs-{lots_id}-label-{index}",
        "name": "label",
        "content": BC_SAMPLE,
        "fontFamily": "Roboto",
        "fontSize": BC_FONT,
        "fontWeight": BC_WEIGHT,
        "lineHeight": BC_LINE_HEIGHT,
        "fill": solid(BC_TEXT if current else BC_SECONDARY),
    }


def crumb_sep(lots_id: str, index: int) -> dict:
    # 4×4 Figma ellipse — rounded frame, not a sibling ellipse
    # (ellipse paints over sibling paths).
    return {
        "type": "frame",
        "id": f"molecule-breadcrumbs-{lots_id}-sep-{index}",
        "name": "sep",
        "width": BC_DOT,
        "height": BC_DOT,
        "cornerRadius": BC_DOT / 2,
        "clipContent": True,
        "fill": solid(BC_SECONDARY),
        "children": [],
    }


def breadcrumbs_frame(variant: dict, y: float) -> dict:
    lots_id = variant["id"]
    count = variant["count"]
    children: list[dict] = []
    for index in range(count):
        if index:
            children.append(crumb_sep(lots_id, index))
        children.append(crumb_label(lots_id, index, current=index == count - 1))
    return {
        "type": "frame",
        "id": f"molecule-breadcrumbs-{lots_id}",
        "name": f"Breadcrumbs/{variant['label']}",
        "reusable": True,
        "x": 0,
        "y": y,
        "width": "fit_content",
        "height": BC_H,
        "layout": "horizontal",
        "gap": BC_GAP,
        "padding": [0, BC_PAD_X],
        "alignItems": "center",
        "cornerRadius": BC_RADIUS,
        "clipContent": False,
        "fill": solid(BC_FILL),
        "stroke": {
            "thickness": 1,
            "fill": solid(BC_BORDER),
        },
        "children": children,
    }


def breadcrumbs_frames() -> list[dict]:
    step = BC_H + BC_SET_GAP
    return [breadcrumbs_frame(variant, index * step) for index, variant in enumerate(LOTS)]


def breadcrumbs_page() -> dict:
    return {
        "id": "page-molecule-breadcrumbs",
        "name": "Molecules · Breadcrumbs",
        "children": breadcrumbs_frames(),
    }
