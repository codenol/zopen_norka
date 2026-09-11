"""Checkbox — 20×20 control, rx 6. Geometry from the 772×280 matrix SVG.

Three selections (Unchecked / Checked / Indeterminate) × five rows
(Default, Hover, Focus, Disabled, Warning) × Icon / Text / TextInfo = 45.

Control path is (2,2)→(22,22) in the matrix (20×20, 1px inside stroke).
Catalog uses a 1px centre stroke like Input — do not set stroke.align
inside. Focus is feMorphology dilate 2 + `$effects/status/focus/default`.
Warning row has no glow in this SVG. Disabled ignores warning (base mute).

Check / dash are evenodd paths (tight ink bbox), not lucide — the SVG
glyphs are not the lucide `check` / `minus` metrics. Live 14/400 «Text».
TextInfo is the 84×24 SVG: gap 8, trailing 16×16 lucide `info`.
Do not author x/y on flex children. Rebuild:
`python3 scripts/merge_skala_atoms.py`.
"""

from __future__ import annotations

BOX = 20.0
BOX_RADIUS = 6.0
STROKE = 1.0
GAP = 8.0
TEXT_H = 20.0
INFO_H = 24.0
INFO_SIZE = 16.0
FONT = 14
WEIGHT = 400
LINE_HEIGHT = round(16 / 14, 4)
LABEL = "Text"
INFO_GLYPH = "info"
SET_GAP = 20.0

# Check in the 20×20 box at matrix (266, 2); dash at (530, 2).
CHECK_D = (
    "M14.862 5.5284C15.123 5.2685 15.545 5.2683 15.805 5.5284"
    "C16.065 5.7888 16.065 6.2115 15.805 6.4718L8.472 13.8048"
    "C8.211 14.0651 7.789 14.0651 7.528 13.8048L4.195 10.4718"
    "C3.935 10.2115 3.935 9.7888 4.195 9.5284C4.456 9.2683"
    " 4.878 9.2684 5.138 9.5284L8 12.3907L14.862 5.5284Z"
)
CHECK_BOX = (4.0, 5.3334, 12.0, 8.6666)
DASH_D = (
    "M14.667 9.333C15.035 9.3332 15.333 9.6319 15.333 10"
    "C15.333 10.3681 15.035 10.6668 14.667 10.667H5.333"
    "C4.965 10.6668 4.667 10.3681 4.667 10C4.667 9.6319"
    " 4.965 9.3332 5.333 9.333H14.667Z"
)
DASH_BOX = (4.667, 9.333, 10.666, 1.334)

SELECTIONS = (
    ("unchecked", "Unchecked"),
    ("checked", "Checked"),
    ("indeterminate", "Indeterminate"),
)
STATES = (
    ("default", "Default"),
    ("hover", "Hover"),
    ("focus", "Focus"),
    ("disabled", "Disabled"),
    ("warning", "Warning"),
)
LAYOUTS = (
    ("icon", "Icon", BOX),
    ("text", "Text", TEXT_H),
    ("text-info", "TextInfo", INFO_H),
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def token(selection: str, layer: str, state: str) -> str:
    return f"$checkbox/{selection}/{layer}/{state}"


def mark_path(box_id: str, selection: str, state: str) -> dict | None:
    if selection == "checked":
        d, (x, y, width, height) = CHECK_D, CHECK_BOX
        name = "check"
    elif selection == "indeterminate":
        d, (x, y, width, height) = DASH_D, DASH_BOX
        name = "dash"
    else:
        return None
    return {
        "type": "path",
        "id": f"{box_id}-{name}",
        "name": name,
        "d": d,
        "x": x,
        "y": y,
        "width": width,
        "height": height,
        "fillRule": "evenodd",
        "fill": solid(token(selection, "icon", state)),
    }


def control_box(box_id: str, selection: str, state: str) -> dict:
    children = []
    mark = mark_path(box_id, selection, state)
    if mark is not None:
        children.append(mark)
    node: dict = {
        "type": "frame",
        "id": box_id,
        "name": "box",
        "width": BOX,
        "height": BOX,
        "layout": "none",
        "cornerRadius": BOX_RADIUS,
        "clipContent": False,
        "fill": solid(token(selection, "background", state)),
        "stroke": {
            "thickness": STROKE,
            "fill": solid(token(selection, "border", state)),
        },
        "children": children,
    }
    if state == "focus":
        # SVG feMorphology dilate 2 + #60C4DD at 30%.
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


def label_node(root_id: str, selection: str, state: str) -> dict:
    return {
        "type": "text",
        "id": f"{root_id}-label",
        "name": "label",
        "content": LABEL,
        "fontFamily": "Roboto",
        "fontSize": FONT,
        "fontWeight": WEIGHT,
        "lineHeight": LINE_HEIGHT,
        "textAlign": "left",
        "fill": solid(token(selection, "text", state)),
    }


def info_icon(root_id: str, selection: str, state: str) -> dict:
    fill = (
        token(selection, "text", "disabled")
        if state == "disabled"
        else "$table/icon/info"
    )
    return {
        "type": "icon_font",
        "id": f"{root_id}-icon",
        "name": "icon",
        "iconFontName": INFO_GLYPH,
        "iconFontFamily": "lucide",
        "width": INFO_SIZE,
        "height": INFO_SIZE,
        "fill": solid(fill),
    }


def checkbox_item(
    selection: str,
    selection_label: str,
    state: str,
    state_label: str,
    layout_id: str,
    layout_label: str,
    height: float,
) -> dict:
    root_id = f"atom-checkbox-{selection}-{state}-{layout_id}"
    name = f"Checkbox/{selection_label}/{state_label}/{layout_label}"
    if layout_id == "icon":
        node = control_box(root_id, selection, state)
        node["name"] = name
        node["reusable"] = True
        return node
    children = [
        control_box(f"{root_id}-box", selection, state),
        label_node(root_id, selection, state),
    ]
    if layout_id == "text-info":
        children.append(info_icon(root_id, selection, state))
    return {
        "type": "frame",
        "id": root_id,
        "name": name,
        "reusable": True,
        "width": "fit_content",
        "height": height,
        "layout": "horizontal",
        "gap": GAP,
        "alignItems": "center",
        "clipContent": False,
        "children": children,
    }


def checkbox_row(state: str, state_label: str) -> dict:
    children = [
        checkbox_item(
            selection,
            selection_label,
            state,
            state_label,
            layout_id,
            layout_label,
            height,
        )
        for selection, selection_label in SELECTIONS
        for layout_id, layout_label, height in LAYOUTS
    ]
    return {
        "type": "frame",
        "id": f"atom-checkbox-row-{state}",
        "name": state_label,
        "layout": "horizontal",
        "gap": SET_GAP,
        "alignItems": "center",
        "clipContent": False,
        "children": children,
    }


def checkbox_page() -> dict:
    return {
        "id": "page-atom-checkbox",
        "name": "Atoms · Checkbox",
        "children": [
            {
                "type": "frame",
                "id": "atom-checkbox-set",
                "name": "set",
                "layout": "vertical",
                "gap": SET_GAP,
                "clipContent": False,
                "children": [
                    checkbox_row(state, label) for state, label in STATES
                ],
            }
        ],
    }
