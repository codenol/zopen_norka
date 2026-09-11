"""Sidebar Avatar — 40×40 tile, 32×32 disc, 4px safe field (not a molecule)."""

from __future__ import annotations

# Figma SVG is 40×40: rect 32×32 at (4, 4) rx=16. The inspector often
# selects only the disc; the atom is the tile so it sits on the same
# 40px minibar grid as Status and MenuButton. Fill alias
# `$sidebar/avatar/background/default` (`#439EE4` / cornflower 550) —
# not `button/primary/info/default-bg` (Dark differs). Initials are
# live 14/500 Roboto, sample «AB». Rounded frame, not a sibling ellipse.
AVATAR_SIZE = 40.0
AVATAR_DISC = 32.0
AVATAR_INSET = (AVATAR_SIZE - AVATAR_DISC) / 2
AVATAR_RADIUS = AVATAR_DISC / 2
AVATAR_FONT = 14
AVATAR_WEIGHT = 500
AVATAR_SAMPLE = "AB"
AVATAR_BG = "$sidebar/avatar/background/default"
AVATAR_FG = "$sidebar/avatar/text/default"


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def avatar_disc() -> dict:
    return {
        "type": "frame",
        "id": "atom-avatar-default-disc",
        "name": "disc",
        "x": AVATAR_INSET,
        "y": AVATAR_INSET,
        "width": AVATAR_DISC,
        "height": AVATAR_DISC,
        "cornerRadius": AVATAR_RADIUS,
        "layout": "horizontal",
        "justifyContent": "center",
        "alignItems": "center",
        "clipContent": True,
        "fill": solid(AVATAR_BG),
        "children": [
            {
                "type": "text",
                "id": "atom-avatar-default-initials",
                "name": "initials",
                "content": AVATAR_SAMPLE,
                "fontFamily": "Roboto",
                "fontSize": AVATAR_FONT,
                "fontWeight": AVATAR_WEIGHT,
                "lineHeight": 1,
                "textAlign": "center",
                "fill": solid(AVATAR_FG),
            }
        ],
    }


def avatar_frame() -> dict:
    return {
        "type": "frame",
        "id": "atom-avatar-default",
        "name": "Avatar/Default",
        "reusable": True,
        "x": 0,
        "y": 0.0,
        "width": AVATAR_SIZE,
        "height": AVATAR_SIZE,
        "layout": "none",
        "clipContent": False,
        "children": [avatar_disc()],
    }


def avatar_page() -> dict:
    return {
        "id": "page-atom-avatar",
        "name": "Atoms · Avatar",
        "children": [avatar_frame()],
    }
