"""Input with icon — 236×32 field, leading or trailing 16×16 lucide.

Geometry from the 496×477 SVG set: two columns (trailing left, leading
right), seven states. Outlined 'Input' / wine / 'hint message' glyphs are
not copied — live text + lucide. Hint under each field is catalog chrome,
not a reusable slot. Do not author x/y on the label or icon.
"""

from __future__ import annotations

from skala_atom_input import (
    INPUT_FONT,
    INPUT_H,
    INPUT_LABEL,
    INPUT_LINE_HEIGHT,
    INPUT_PAD_X,
    INPUT_PAD_Y,
    INPUT_RADIUS,
    INPUT_STROKE,
    INPUT_W,
    INPUT_WEIGHT,
    STATES,
    solid,
    token,
)

ICON_SIZE = 16.0
ICON_GAP = 8.0
# Figma sample glyph is wine (not in Icon::from_name); catalog uses map-pin.
SAMPLE_ICON = "map-pin"
HINT_TEXT = "hint message"
HINT_FONT = 12
HINT_LINE_HEIGHT = round(16 / 12, 4)
HINT_GAP = 8.0
COLUMN_GAP = 20.0
SAMPLE_GAP = 20.0

PLACEMENTS = (
    {"id": "trailing", "label": "Trailing"},
    {"id": "leading", "label": "Leading"},
)


def icon_token(state_id: str) -> str:
    return token("icon", state_id)


def hint_token(state_id: str) -> str:
    # Warning/error hint ink matches the field stroke in the SVG.
    if state_id in {"warning", "error"}:
        return token("border", state_id)
    return token("text", state_id)


def lucide_icon(icon_id: str, fill: str) -> dict:
    return {
        "type": "icon_font",
        "id": icon_id,
        "name": "icon",
        "iconFontName": SAMPLE_ICON,
        "iconFontFamily": "lucide",
        "width": ICON_SIZE,
        "height": ICON_SIZE,
        "fill": solid(fill),
    }


def input_label(root_id: str, state_id: str) -> dict:
    return {
        "type": "text",
        "id": f"{root_id}-label",
        "name": "label",
        "content": INPUT_LABEL,
        "fontFamily": "Roboto",
        "fontSize": INPUT_FONT,
        "fontWeight": INPUT_WEIGHT,
        "lineHeight": INPUT_LINE_HEIGHT,
        "textAlign": "left",
        "fill": solid(token("text", state_id)),
    }


def hint_label(root_id: str, state_id: str) -> dict:
    return {
        "type": "text",
        "id": f"{root_id}-hint",
        "name": "hint",
        "content": HINT_TEXT,
        "fontFamily": "Roboto",
        "fontSize": HINT_FONT,
        "fontWeight": INPUT_WEIGHT,
        "lineHeight": HINT_LINE_HEIGHT,
        "textAlign": "left",
        "fill": solid(hint_token(state_id)),
    }


def input_icon_frame(placement: dict, state: dict) -> dict:
    state_id = state["id"]
    root_id = f"atom-input-icon-{placement['id']}-{state_id}"
    label = input_label(root_id, state_id)
    icon = lucide_icon(f"{root_id}-icon", icon_token(state_id))
    if placement["id"] == "leading":
        children = [icon, label]
        justify = "start"
        gap = ICON_GAP
    else:
        children = [label, icon]
        justify = "space_between"
        gap = ICON_GAP

    node: dict = {
        "type": "frame",
        "id": root_id,
        "name": f"Input/Icon/{placement['label']}/{state['label']}",
        "reusable": True,
        "width": INPUT_W,
        "height": INPUT_H,
        "layout": "horizontal",
        "gap": gap,
        "padding": [INPUT_PAD_Y, INPUT_PAD_X],
        "justifyContent": justify,
        "alignItems": "center",
        "cornerRadius": INPUT_RADIUS,
        "clipContent": False,
        "fill": solid(token("background", state_id)),
        "stroke": {
            "thickness": INPUT_STROKE,
            "fill": solid(token("border", state_id)),
        },
        "children": children,
    }
    if state_id == "focused":
        node["effects"] = [
            {
                "type": "shadow",
                "offsetX": 0,
                "offsetY": 0,
                "blur": 0,
                "spread": 2,
                "color": "$effects/status/focus/default",
            }
        ]
    return node


def sample_group(placement: dict, state: dict) -> dict:
    field = input_icon_frame(placement, state)
    return {
        "type": "frame",
        "id": f"atom-input-icon-sample-{placement['id']}-{state['id']}",
        "name": f"{placement['label']} {state['label']}",
        "layout": "vertical",
        "gap": HINT_GAP,
        "clipContent": False,
        "children": [field, hint_label(field["id"], state["id"])],
    }


def placement_column(placement: dict) -> dict:
    return {
        "type": "frame",
        "id": f"atom-input-icon-col-{placement['id']}",
        "name": placement["label"],
        "layout": "vertical",
        "gap": SAMPLE_GAP,
        "clipContent": False,
        "children": [sample_group(placement, state) for state in STATES],
    }


def input_icon_page() -> dict:
    return {
        "id": "page-atom-input-icon",
        "name": "Atoms · Input icon",
        "children": [
            {
                "type": "frame",
                "id": "atom-input-icon-set",
                "name": "set",
                "layout": "horizontal",
                "gap": COLUMN_GAP,
                "clipContent": False,
                "children": [placement_column(p) for p in PLACEMENTS],
            }
        ],
    }
