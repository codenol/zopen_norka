"""Chip — dismissible 20px pill. Geometry from the 60×60 SVG set.

Two tiles (Default y=0, Disabled y=40), catalog gap 20. Shell 60×20 at
sample «Text», rx 10 (half height, not `$border-radius/S`). Clip is a
12×12 slot at (8, 4): lucide `x-circle`, gap 8, pad 4/8. Live 12/400.
Icon tokens `$chip/icon/*` match the SVG (`#75777B` / `#DFE2E6` in Light).
"""

from __future__ import annotations

CHIP_H = 20.0
CHIP_RADIUS = 10.0
CHIP_PAD_Y = 4.0
CHIP_PAD_X = 8.0
CHIP_GAP = 8.0
CHIP_ICON = 12.0
CHIP_FONT = 12
CHIP_WEIGHT = 400
CHIP_LINE_HEIGHT = 1.0
CHIP_LABEL = "Text"
CHIP_GLYPH = "x-circle"
CHIP_SET_GAP = 20.0

STATES = (
    ("default", "Default"),
    ("disabled", "Disabled"),
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def lucide_icon(root_id: str, state_id: str) -> dict:
    return {
        "type": "icon_font",
        "id": f"{root_id}-icon",
        "name": "icon",
        "iconFontName": CHIP_GLYPH,
        "iconFontFamily": "lucide",
        "width": CHIP_ICON,
        "height": CHIP_ICON,
        "fill": solid(f"$chip/icon/{state_id}"),
    }


def chip_label(root_id: str, state_id: str) -> dict:
    return {
        "type": "text",
        "id": f"{root_id}-label",
        "name": "label",
        "content": CHIP_LABEL,
        "fontFamily": "Roboto",
        "fontSize": CHIP_FONT,
        "fontWeight": CHIP_WEIGHT,
        "lineHeight": CHIP_LINE_HEIGHT,
        "textAlign": "left",
        "fill": solid(f"$chip/text/{state_id}"),
    }


def chip_item(state_id: str, state_label: str, y: float) -> dict:
    root_id = f"atom-chip-{state_id}"
    return {
        "type": "frame",
        "id": root_id,
        "name": f"Chip/{state_label}",
        "reusable": True,
        "x": 0,
        "y": y,
        "width": "fit_content",
        "height": CHIP_H,
        "layout": "horizontal",
        "gap": CHIP_GAP,
        "padding": [CHIP_PAD_Y, CHIP_PAD_X],
        "justifyContent": "start",
        "alignItems": "center",
        "cornerRadius": CHIP_RADIUS,
        "clipContent": False,
        "fill": solid(f"$chip/background/{state_id}"),
        "children": [
            lucide_icon(root_id, state_id),
            chip_label(root_id, state_id),
        ],
    }


def chip_page() -> dict:
    return {
        "id": "page-atom-chip",
        "name": "Atoms · Chip",
        "children": [
            chip_item(state_id, label, index * (CHIP_H + CHIP_SET_GAP))
            for index, (state_id, label) in enumerate(STATES)
        ],
    }
