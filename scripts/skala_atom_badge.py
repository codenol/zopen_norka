"""Color Badge — 17px VALUE tags. Geometry from the 334×451 SVG matrix.

Figma property panel says Chip; tokens are `$badge/{color}/…`. Not the
dismissible Chip atom. 15 colors × filled/outline × text/text+icon/icon = 90.

Row step 31 (h 17, catalog gap 14). Columns x=0 / 58 / 132 / 182.5 / 240.5 /
316.5. Filled Text+Icon puts the icon after the label; Outline TextIcon puts
it before (SVG order). Figma wine glyph is not in `Icon::from_name`; catalog
uses `map-pin` like Button. Live 11/500 «VALUE».
"""

from __future__ import annotations

BADGE_H = 17.0
BADGE_RADIUS = 8.5
BADGE_PAD_Y = 3.0
BADGE_PAD_X = 6.0
BADGE_ICON_PAD_X = 4.0
BADGE_GAP = 4.0
BADGE_ICON = 10.0
BADGE_FONT = 11
BADGE_WEIGHT = 500
BADGE_LINE_HEIGHT = 1.0
BADGE_LABEL = "VALUE"
BADGE_GLYPH = "map-pin"
ROW_STEP = 31.0

# SVG column origins (filled group, then outline group).
COL_X = {
    ("filled", "text"): 0.0,
    ("filled", "text-icon"): 58.0,
    ("filled", "icon"): 132.0,
    ("outline", "text"): 182.5,
    ("outline", "text-icon"): 240.5,
    ("outline", "icon"): 316.5,
}

# SVG fill order, top to bottom.
COLORS = (
    "gray-strong",
    "gray",
    "nile-blue",
    "tory-blue",
    "cornflower-blue",
    "bondi-blue",
    "java",
    "green",
    "shamrock",
    "yellow",
    "orange",
    "red",
    "rose",
    "violet",
    "gray-warm",
)

KINDS = (
    ("filled", "Filled"),
    ("outline", "Outline"),
)
LAYOUTS = (
    ("text", "Text"),
    ("text-icon", "TextIcon"),
    ("icon", "Icon"),
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def token(color: str, *parts: str) -> str:
    return "$badge/" + "/".join((color, *parts))


def lucide_icon(root_id: str, color: str) -> dict:
    return {
        "type": "icon_font",
        "id": f"{root_id}-icon",
        "name": "icon",
        "iconFontName": BADGE_GLYPH,
        "iconFontFamily": "lucide",
        "width": BADGE_ICON,
        "height": BADGE_ICON,
        "fill": solid(token(color, "icon", "default")),
    }


def badge_label(root_id: str, color: str) -> dict:
    return {
        "type": "text",
        "id": f"{root_id}-label",
        "name": "label",
        "content": BADGE_LABEL,
        "fontFamily": "Roboto",
        "fontSize": BADGE_FONT,
        "fontWeight": BADGE_WEIGHT,
        "lineHeight": BADGE_LINE_HEIGHT,
        "textAlign": "left",
        "fill": solid(token(color, "text", "default")),
    }


def layout_children(root_id: str, color: str, kind: str, layout: str) -> list[dict]:
    icon = lucide_icon(root_id, color)
    label = badge_label(root_id, color)
    if layout == "text":
        return [label]
    if layout == "icon":
        return [icon]
    # text-icon: filled = label then icon; outline = icon then label (SVG).
    if kind == "outline":
        return [icon, label]
    return [label, icon]


def badge_item(
    color: str,
    kind: str,
    kind_label: str,
    layout: str,
    layout_label: str,
    x: float,
    y: float,
) -> dict:
    root_id = f"atom-badge-{color}-{kind}-{layout}"
    fill_layer = "filled/background" if kind == "filled" else "outline/background"
    node = {
        "type": "frame",
        "id": root_id,
        "name": f"Badge/{color}/{kind_label}/{layout_label}",
        "reusable": True,
        "x": x,
        "y": y,
        "width": "fit_content",
        "height": BADGE_H,
        "layout": "horizontal",
        "gap": 0 if layout in {"text", "icon"} else BADGE_GAP,
        "padding": [
            BADGE_PAD_Y,
            BADGE_ICON_PAD_X if layout == "icon" else BADGE_PAD_X,
        ],
        "justifyContent": "center",
        "alignItems": "center",
        "cornerRadius": BADGE_RADIUS,
        "clipContent": False,
        "fill": solid(token(color, fill_layer, "default")),
        "children": layout_children(root_id, color, kind, layout),
    }
    if kind == "outline":
        node["stroke"] = {
            "thickness": 1,
            "fill": solid(token(color, "outline/border", "default")),
        }
    return node


def badge_frames() -> list[dict]:
    frames = []
    for row, color in enumerate(COLORS):
        y = row * ROW_STEP
        for kind, kind_label in KINDS:
            for layout, layout_label in LAYOUTS:
                frames.append(
                    badge_item(
                        color,
                        kind,
                        kind_label,
                        layout,
                        layout_label,
                        COL_X[(kind, layout)],
                        y,
                    )
                )
    return frames


def badge_page() -> dict:
    return {
        "id": "page-atom-badge",
        "name": "Atoms · Badge",
        "children": badge_frames(),
    }
