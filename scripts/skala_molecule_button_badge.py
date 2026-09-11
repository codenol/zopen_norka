"""Button with badge — Large Accent Text + Badge/Basic overlay.

Geometry from the 85×92 SVG: button 83×32 at (0, 8), badge 16×16 at (69, 0).
Catalog gap 20 to a plain Button (not a second master). Wrapper is layout
none / clipContent false so the circle can sit on the top-right edge.
"""

from __future__ import annotations

WRAP_W = 85.0
WRAP_H = 40.0
BUTTON_Y = 8.0
BADGE_X = 69.0
BADGE_Y = 0.0
PLAIN_Y = 60.0
BUTTON_REF = "atom-button-filled-large-accent-default-text"
BADGE_REF = "atom-badge-basic-base"


def instance(node_id: str, target: str, x: float, y: float) -> dict:
    return {"type": "ref", "id": node_id, "ref": target, "x": x, "y": y}


def button_with_badge() -> dict:
    return {
        "type": "frame",
        "id": "molecule-button-with-badge",
        "name": "Button/WithBadge",
        "reusable": True,
        "x": 0,
        "y": 0,
        "width": WRAP_W,
        "height": WRAP_H,
        "layout": "none",
        "clipContent": False,
        "children": [
            instance(
                "molecule-button-with-badge-button",
                BUTTON_REF,
                0,
                BUTTON_Y,
            ),
            instance(
                "molecule-button-with-badge-badge",
                BADGE_REF,
                BADGE_X,
                BADGE_Y,
            ),
        ],
    }


def button_badge_page() -> dict:
    return {
        "id": "page-molecule-button-badge",
        "name": "Molecules · ButtonBadge",
        "children": [
            button_with_badge(),
            {
                "type": "ref",
                "id": "page-molecule-button-badge-plain",
                "ref": BUTTON_REF,
                "x": 0,
                "y": PLAIN_Y,
            },
        ],
    }
