"""Pagination items — 32×32 number / ellipsis / arrow, plus the page-size trigger.

Geometry from the Figma SVGs (32×136 numbers, 77×131 arrows, 60×32
page-size chrome). Catalog gap 20. Radius `$border-radius/M`. Default
number/arrow fills are transparent (`white/0-t` via the pagination
tokens). Page-size menu is ContextMenu/PageSize; this atom is the
closed trigger only.
"""

from __future__ import annotations

ITEM = 32.0
ITEM_RADIUS = 8.0
ITEM_ICON = 16.0
ITEM_SET_GAP = 20.0
ITEM_COL_GAP = 16.0
ITEM_FONT = 14
ITEM_WEIGHT = 400
ITEM_LINE_HEIGHT = round(16 / 14, 4)
PAGE_SIZE_H = 32.0
PAGE_SIZE_RADIUS = 8.0
PAGE_SIZE_PAD_Y = 8.0
PAGE_SIZE_PAD_X = 12.0
PAGE_SIZE_GAP = 8.0
PAGE_SIZE_SAMPLE = "10"
PAGE_SIZE_ICON = "chevron-down"


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def lucide_icon(node_id: str, glyph: str, fill: str) -> dict:
    return {
        "type": "icon_font",
        "id": node_id,
        "name": "icon",
        "iconFontName": glyph,
        "iconFontFamily": "lucide",
        "width": ITEM_ICON,
        "height": ITEM_ICON,
        "fill": solid(fill),
    }


def number_label(state_id: str, sample: str) -> dict:
    return {
        "type": "text",
        "id": f"atom-pagination-item-number-{state_id}-label",
        "name": "label",
        "content": sample,
        "fontFamily": "Roboto",
        "fontSize": ITEM_FONT,
        "fontWeight": ITEM_WEIGHT,
        "lineHeight": ITEM_LINE_HEIGHT,
        "textAlign": "center",
        "fill": solid(f"$pagination/item/number/text/{state_id}"),
    }


def number_item(state_id: str, state_label: str, sample: str, y: float) -> dict:
    return {
        "type": "frame",
        "id": f"atom-pagination-item-number-{state_id}",
        "name": f"Pagination/Item/Number/{state_label}",
        "reusable": True,
        "x": 0,
        "y": y,
        "width": ITEM,
        "height": ITEM,
        "layout": "horizontal",
        "justifyContent": "center",
        "alignItems": "center",
        "cornerRadius": ITEM_RADIUS,
        "clipContent": False,
        "fill": solid(f"$pagination/item/number/background/{state_id}"),
        "children": [number_label(state_id, sample)],
    }


def ellipsis_item(y: float) -> dict:
    return {
        "type": "frame",
        "id": "atom-pagination-item-ellipsis-default",
        "name": "Pagination/Item/Ellipsis/Default",
        "reusable": True,
        "x": 0,
        "y": y,
        "width": ITEM,
        "height": ITEM,
        "layout": "horizontal",
        "justifyContent": "center",
        "alignItems": "center",
        "cornerRadius": ITEM_RADIUS,
        "clipContent": False,
        "fill": solid("$pagination/item/ellipsis/background/default"),
        "children": [
            lucide_icon(
                "atom-pagination-item-ellipsis-default-icon",
                "ellipsis",
                "$pagination/item/ellipsis/icon/default",
            )
        ],
    }


def arrow_item(direction: str, direction_label: str, state_id: str, state_label: str, x: float, y: float) -> dict:
    glyph = "chevron-left" if direction == "prev" else "chevron-right"
    root_id = f"atom-pagination-item-arrow-{direction}-{state_id}"
    return {
        "type": "frame",
        "id": root_id,
        "name": f"Pagination/Item/Arrow/{direction_label}/{state_label}",
        "reusable": True,
        "x": x,
        "y": y,
        "width": ITEM,
        "height": ITEM,
        "layout": "horizontal",
        "justifyContent": "center",
        "alignItems": "center",
        "cornerRadius": ITEM_RADIUS,
        "clipContent": False,
        "fill": solid(f"$pagination/item/arrow/background/{state_id}"),
        "children": [
            lucide_icon(
                f"{root_id}-icon",
                glyph,
                f"$pagination/item/arrow/icon/{state_id}",
            )
        ],
    }


def pagesize_item(y: float) -> dict:
    root_id = "atom-pagination-pagesize-default"
    return {
        "type": "frame",
        "id": root_id,
        "name": "Pagination/PageSize/Default",
        "reusable": True,
        "x": 0,
        "y": y,
        "width": "fit_content",
        "height": PAGE_SIZE_H,
        "layout": "horizontal",
        "gap": PAGE_SIZE_GAP,
        "padding": [PAGE_SIZE_PAD_Y, PAGE_SIZE_PAD_X],
        "justifyContent": "center",
        "alignItems": "center",
        "cornerRadius": PAGE_SIZE_RADIUS,
        "clipContent": False,
        "fill": solid("$dropdown/background/default"),
        "stroke": {
            "thickness": 1,
            "fill": solid("$dropdown/border/default"),
        },
        "children": [
            {
                "type": "text",
                "id": f"{root_id}-label",
                "name": "label",
                "content": PAGE_SIZE_SAMPLE,
                "fontFamily": "Roboto",
                "fontSize": ITEM_FONT,
                "fontWeight": ITEM_WEIGHT,
                "lineHeight": ITEM_LINE_HEIGHT,
                "textAlign": "left",
                "fill": solid("$dropdown/text/default"),
            },
            lucide_icon(f"{root_id}-icon", PAGE_SIZE_ICON, "$dropdown/icon/default"),
        ],
    }


def pagination_atom_frames() -> list[dict]:
    step = ITEM + ITEM_SET_GAP
    numbers = (
        ("default", "Default", "1"),
        ("hover", "Hover", "1"),
        ("selected", "Selected", "1"),
    )
    frames = [
        number_item(state_id, label, sample, index * step)
        for index, (state_id, label, sample) in enumerate(numbers)
    ]
    ellipsis_y = len(numbers) * step
    frames.append(ellipsis_item(ellipsis_y))
    arrow_states = (
        ("default", "Default"),
        ("hover", "Hover"),
        ("disabled", "Disabled"),
    )
    arrow_origin = ellipsis_y + step
    for index, (state_id, label) in enumerate(arrow_states):
        y = arrow_origin + index * step
        frames.append(arrow_item("prev", "Prev", state_id, label, 0.0, y))
        frames.append(
            arrow_item("next", "Next", state_id, label, ITEM + ITEM_COL_GAP, y)
        )
    pagesize_y = arrow_origin + len(arrow_states) * step
    frames.append(pagesize_item(pagesize_y))
    return frames


def pagination_atom_page() -> dict:
    return {
        "id": "page-atom-pagination",
        "name": "Atoms · Pagination",
        "children": pagination_atom_frames(),
    }
