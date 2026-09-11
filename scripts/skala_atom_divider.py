"""Sidebar Divider — 1px hairline (not a molecule)."""

from __future__ import annotations

# Figma instance is 200 Fill × 1 Hug with a Fill (not Stroke) of
# `$sidebar/border/default`. Do not bind `$sidebar/divider-horizontal/default`:
# that alias is cool-gray-800 (`#3F4146`) in Light, not the hairline.
DIVIDER_W = 200.0
DIVIDER_H = 1.0
DIVIDER_FILL = "$sidebar/border/default"


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def divider_frame() -> dict:
    return {
        "type": "frame",
        "id": "atom-divider-default",
        "name": "Divider/Default",
        "reusable": True,
        "x": 0,
        "y": 0.0,
        "width": DIVIDER_W,
        "height": DIVIDER_H,
        "layout": "none",
        "clipContent": False,
        "fill": solid(DIVIDER_FILL),
        "children": [],
    }


def divider_page() -> dict:
    return {
        "id": "page-atom-divider",
        "name": "Atoms · Divider",
        "children": [divider_frame()],
    }
