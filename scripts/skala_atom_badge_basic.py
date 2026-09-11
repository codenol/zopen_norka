"""Badge-basic — 16px count circle. Geometry from the 45×16 SVG set.

Two tiles (Base x=0, MassAction x=29), catalog gap 13. Circle 16×16, rx 8,
1px centre white stroke. Live 10/500 «1» — SVG outlines the digit; do not
keep paths. Tokens `$badge-basic/{base,massAction}/{background,stroke,text}/default`.
"""

from __future__ import annotations

BADGE_SIZE = 16.0
BADGE_RADIUS = 8.0
BADGE_FONT = 10
BADGE_WEIGHT = 500
BADGE_LINE_HEIGHT = 1.0
BADGE_LABEL = "1"
BADGE_SET_GAP = 13.0

VARIANTS = (
    ("base", "Base"),
    ("mass-action", "MassAction"),
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def token_kind(variant_id: str) -> str:
    return "massAction" if variant_id == "mass-action" else variant_id


def badge_label(root_id: str, kind: str) -> dict:
    return {
        "type": "text",
        "id": f"{root_id}-label",
        "name": "label",
        "content": BADGE_LABEL,
        "fontFamily": "Roboto",
        "fontSize": BADGE_FONT,
        "fontWeight": BADGE_WEIGHT,
        "lineHeight": BADGE_LINE_HEIGHT,
        "textAlign": "center",
        "fill": solid(f"$badge-basic/{kind}/text/default"),
    }


def badge_item(variant_id: str, variant_label: str, x: float) -> dict:
    kind = token_kind(variant_id)
    root_id = f"atom-badge-basic-{variant_id}"
    return {
        "type": "frame",
        "id": root_id,
        "name": f"Badge/Basic/{variant_label}",
        "reusable": True,
        "x": x,
        "y": 0,
        "width": BADGE_SIZE,
        "height": BADGE_SIZE,
        "layout": "horizontal",
        "justifyContent": "center",
        "alignItems": "center",
        "cornerRadius": BADGE_RADIUS,
        "clipContent": False,
        "fill": solid(f"$badge-basic/{kind}/background/default"),
        "stroke": {
            "thickness": 1,
            "fill": solid(f"$badge-basic/{kind}/stroke/default"),
        },
        "children": [badge_label(root_id, kind)],
    }


def badge_basic_page() -> dict:
    return {
        "id": "page-atom-badge-basic",
        "name": "Atoms · BadgeBasic",
        "children": [
            badge_item(
                variant_id,
                label,
                index * (BADGE_SIZE + BADGE_SET_GAP),
            )
            for index, (variant_id, label) in enumerate(VARIANTS)
        ],
    }
