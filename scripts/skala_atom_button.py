"""Button — hug control. Geometry from Filled Accent Default SVG (Large + Small)."""

from __future__ import annotations

# Large SVG 351×32: text 83, icon-start/end 105, icon-only 32.
# Small SVG 277×24: text 61, icon-start/end 77, icon-only 24.
# Outline Large SVG is the same boxes with 1px centre stroke (path inset 0.5),
# no fill, ink Bondi — not align: inside.
# Catalog row gap 8. Radius 8 both sizes.
# Figma sample glyph is wine (not in Icon::from_name); catalog uses map-pin.
# Ghost Large SVG 351×32: same hug, no chrome — only Bondi content paths.
# Ghost Small SVG 281×24: same 61 / 77 / 24, 12×12 wine, no chrome.
# Type Text waits for its own SVG.

BTN_RADIUS = 8.0
BTN_SET_GAP = 8.0
BTN_WEIGHT = 500
BTN_LABEL = "Button"
BTN_SAMPLE_ICON = "map-pin"

SIZES = {
    "large": {
        "label": "Large",
        "height": 32.0,
        "pad_y": 8.0,
        "pad_x": 16.0,
        "pad_icon": 8.0,
        "gap": 8.0,
        "icon": 16.0,
        "font": 14,
        "line_height": round(16 / 14, 4),
    },
    "small": {
        "label": "Small",
        "height": 24.0,
        "pad_y": 6.0,
        "pad_x": 8.0,
        "pad_icon": 6.0,
        "gap": 4.0,
        "icon": 12.0,
        "font": 12,
        "line_height": 1.0,
    },
}

TYPES = (
    ("filled", "Filled"),
    ("outline", "Outline"),
    ("ghost", "Ghost"),
)
SENTIMENTS = (
    ("accent", "Accent"),
    ("danger", "Danger"),
    ("warning", "Warning"),
    ("success", "Success"),
    ("info", "Info"),
    ("secondary", "Secondary"),
)
STATES = (
    ("default", "Default"),
    ("hover", "Hover"),
    ("active", "Active"),
    ("focus", "Focus"),
    ("disabled", "Disabled"),
)
LAYOUTS = (
    ("text", None),
    ("icon-start", "Icon start"),
    ("icon-end", "Icon end"),
    ("icon", "Icon"),
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def token(kind: str, sentiment: str, layer: str, state: str) -> str:
    return f"$button/{kind}/{sentiment}/{layer}/{state}"


def node_id(kind: str, size: str, sentiment: str, state: str, layout: str) -> str:
    return f"atom-button-{kind}-{size}-{sentiment}-{state}-{layout}"


def display_name(
    kind: str, size: str, sentiment_label: str, state_label: str, layout_label: str | None
) -> str:
    name = f"Button/{kind.title()}/{size.title()}/{sentiment_label}/{state_label}"
    if layout_label:
        name = f"{name}/{layout_label}"
    return name


def lucide_icon(icon_id: str, fill: str, icon_size: float) -> dict:
    return {
        "type": "icon_font",
        "id": icon_id,
        "name": "icon",
        "iconFontName": BTN_SAMPLE_ICON,
        "iconFontFamily": "lucide",
        "width": icon_size,
        "height": icon_size,
        "fill": solid(fill),
    }


def label_node(label_id: str, fill: str, spec: dict) -> dict:
    return {
        "type": "text",
        "id": label_id,
        "name": "label",
        "content": BTN_LABEL,
        "fontFamily": "Roboto",
        "fontSize": spec["font"],
        "fontWeight": BTN_WEIGHT,
        "lineHeight": spec["line_height"],
        "textAlign": "center",
        "fill": solid(fill),
    }


def button(
    kind: str,
    size: str,
    sentiment: str,
    sentiment_label: str,
    state: str,
    state_label: str,
    layout: str,
    layout_label: str | None,
) -> dict:
    spec = SIZES[size]
    root_id = node_id(kind, size, sentiment, state, layout)
    ink = token(kind, sentiment, "text", state)
    icon_size = spec["icon"]
    if layout == "text":
        children = [label_node(f"{root_id}-label", ink, spec)]
    elif layout == "icon-start":
        children = [
            lucide_icon(f"{root_id}-icon", ink, icon_size),
            label_node(f"{root_id}-label", ink, spec),
        ]
    elif layout == "icon-end":
        children = [
            label_node(f"{root_id}-label", ink, spec),
            lucide_icon(f"{root_id}-icon", ink, icon_size),
        ]
    else:
        children = [lucide_icon(f"{root_id}-icon", ink, icon_size)]

    pad_x = spec["pad_icon"] if layout == "icon" else spec["pad_x"]
    height = spec["height"]
    node: dict = {
        "type": "frame",
        "id": root_id,
        "name": display_name(kind, size, sentiment_label, state_label, layout_label),
        "reusable": True,
        "width": height if layout == "icon" else "fit_content",
        "height": height,
        "layout": "horizontal",
        "gap": 0 if layout in {"text", "icon"} else spec["gap"],
        "padding": [spec["pad_y"], pad_x],
        "justifyContent": "center",
        "alignItems": "center",
        "cornerRadius": BTN_RADIUS,
        "clipContent": False,
        "fill": solid(token(kind, sentiment, "background", state)),
        "children": children,
    }
    # Filled/ghost default: no ring. Outline: 1px centre stroke. Focus: ring
    # on every type (ghost/filled border/focus is Bondi-500). Not align: inside.
    if kind == "outline" or state == "focus":
        node["stroke"] = {
            "thickness": 1,
            "fill": solid(token(kind, sentiment, "border", state)),
        }
    return node


def button_row(
    kind: str,
    size: str,
    sentiment: str,
    sentiment_label: str,
    state: str,
    state_label: str,
) -> dict:
    return {
        "type": "frame",
        "id": f"atom-button-row-{kind}-{size}-{sentiment}-{state}",
        "name": f"{sentiment_label} {state_label}",
        "layout": "horizontal",
        "gap": BTN_SET_GAP,
        "alignItems": "center",
        "children": [
            button(
                kind,
                size,
                sentiment,
                sentiment_label,
                state,
                state_label,
                layout,
                layout_label,
            )
            for layout, layout_label in LAYOUTS
        ],
    }


def button_page() -> dict:
    groups = []
    for kind, kind_label in TYPES:
        for size, spec in SIZES.items():
            for sentiment, sentiment_label in SENTIMENTS:
                rows = [
                    button_row(kind, size, sentiment, sentiment_label, state, state_label)
                    for state, state_label in STATES
                ]
                groups.append(
                    {
                        "type": "frame",
                        "id": f"atom-button-group-{kind}-{size}-{sentiment}",
                        "name": f"{kind_label} {spec['label']} {sentiment_label}",
                        "layout": "vertical",
                        "gap": BTN_SET_GAP,
                        "children": rows,
                    }
                )
    return {
        "id": "page-atom-button",
        "name": "Atoms · Button",
        "children": [
            {
                "type": "frame",
                "id": "atom-button-set",
                "name": "set",
                "layout": "vertical",
                "gap": 16.0,
                "children": groups,
            }
        ],
    }
