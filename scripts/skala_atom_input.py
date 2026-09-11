"""Input — 236×32 hug field, seven Figma states. Geometry from the SVG set."""

from __future__ import annotations

# SVG set is 240×344: seven 236×32 fields, 20px row gap (tops at 0, 52, …).
# Inspector chrome and the filled-state lavender selection ring are not part
# of the atom. Trust the SVG: default/filled share cool-gray-200 stroke;
# hover Bondi-600; focused Bondi-500 + 2px dilate glow at 30%.
# Do not author x/y on the label — explicit 0,0 is Position::Absolute.

INPUT_W = 236.0
INPUT_H = 32.0
INPUT_RADIUS = 8.0
INPUT_PAD_Y = 8.0
INPUT_PAD_X = 12.0
INPUT_FONT = 14
INPUT_WEIGHT = 400
INPUT_LINE_HEIGHT = round(16 / 14, 4)
INPUT_SET_GAP = 20.0
INPUT_LABEL = "Input"
INPUT_STROKE = 1.0

STATES = (
    {"id": "default", "label": "Default"},
    {"id": "hover", "label": "Hover"},
    {"id": "focused", "label": "Focused"},
    {"id": "filled", "label": "Filled"},
    {"id": "warning", "label": "Warning"},
    {"id": "error", "label": "Error"},
    {"id": "disabled", "label": "Disabled"},
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def token(layer: str, state: str) -> str:
    return f"$input/{layer}/{state}"


def input_label(state_id: str) -> dict:
    return {
        "type": "text",
        "id": f"atom-input-{state_id}-label",
        "name": "label",
        "content": INPUT_LABEL,
        "fontFamily": "Roboto",
        "fontSize": INPUT_FONT,
        "fontWeight": INPUT_WEIGHT,
        "lineHeight": INPUT_LINE_HEIGHT,
        "textAlign": "left",
        "fill": solid(token("text", state_id)),
    }


def input_frame(state: dict) -> dict:
    node = {
        "type": "frame",
        "id": f"atom-input-{state['id']}",
        "name": f"Input/{state['label']}",
        "reusable": True,
        "x": 0,
        "y": 0,
        "width": INPUT_W,
        "height": INPUT_H,
        "layout": "horizontal",
        "padding": [INPUT_PAD_Y, INPUT_PAD_X],
        "alignItems": "center",
        "cornerRadius": INPUT_RADIUS,
        "clipContent": False,
        "fill": solid(token("background", state["id"])),
        "stroke": {
            "thickness": INPUT_STROKE,
            "fill": solid(token("border", state["id"])),
        },
        "children": [input_label(state["id"])],
    }
    if state["id"] == "focused":
        # SVG feMorphology dilate 2 + #60C4DD at 30% → token 500-t-30.
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


def input_page() -> dict:
    return {
        "id": "page-atom-input",
        "name": "Atoms · Input",
        "children": [
            {
                "type": "frame",
                "id": "atom-input-set",
                "name": "set",
                "layout": "vertical",
                "gap": INPUT_SET_GAP,
                "clipContent": False,
                "children": [input_frame(state) for state in STATES],
            }
        ],
    }
