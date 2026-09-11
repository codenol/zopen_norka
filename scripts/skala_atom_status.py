"""Sidebar Status dots (not StatusIndicator)."""

from __future__ import annotations

# Figma set is 40×132: three 40×40 tiles, 6px gap. Inspector 80×172 is chrome.
STATUS_SIZE = 40.0
STATUS_GAP = 6.0
STATUS_DOT = 20.0
STATUS_INSET = (STATUS_SIZE - STATUS_DOT) / 2

# Figma instance `lock-keyhole` is 12×12 at (14, 14) in the 40×40 tile,
# i.e. (4, 4) inside the 20×20 dot. Path `d` is local to that slot.
# Path node stays the tight ink bbox — do not author it as 12×12 or the
# renderer stretches the shackle. The dot is a rounded frame, not an
# ellipse: a sibling ellipse paints over the path and hides the glyph.
STATUS_ICON_SIZE = 12.0
STATUS_ICON_X = 4.0
STATUS_ICON_Y = 4.0
STATUS_LOCK_D = (
    "M6 0.5C6.7956 0.5 7.5585 0.8163 8.1211 1.3789"
    "C8.6837 1.9415 9 2.7044 9 3.5V4.5H9.5C10.3284 4.5 11 5.1716"
    " 11 6V10C11 10.8284 10.3284 11.5 9.5 11.5H2.5C1.6716 11.5 1 10.8284"
    " 1 10V6C1 5.1716 1.6716 4.5 2.5 4.5H3V3.5C3 2.7044 3.3163"
    " 1.9415 3.8789 1.3789C4.4415 0.8163 5.2044 0.5 6 0.5ZM2.5 5.5"
    "C2.2239 5.5 2 5.7239 2 6V10C2 10.2761 2.2239 10.5 2.5 10.5H9.5"
    "C9.7761 10.5 10 10.2761 10 10V6C10 5.7239 9.7761 5.5 9.5 5.5H2.5"
    "ZM6 7C6.5523 7 7 7.4477 7 8C7 8.5523 6.5523 9 6 9C5.4477"
    " 9 5 8.5523 5 8C5 7.4477 5.4477 7 6 7ZM6 1.5C5.4696 1.5"
    " 4.961 1.7109 4.5859 2.0859C4.2109 2.461 4 2.9696 4 3.5V4.5"
    "H8V3.5C8 2.9696 7.7891 2.461 7.4141 2.0859C7.039 1.7109"
    " 6.5304 1.5 6 1.5Z"
)
STATUS_LOCK_X = 1.0
STATUS_LOCK_Y = 0.5
STATUS_LOCK_W = 10.0
STATUS_LOCK_H = 11.0

STATUS_VARIANTS = (
    {
        "id": "default",
        "label": "Default",
        "fill": "$sidebar/status/indicator/default",
    },
    {
        "id": "lock",
        "label": "Lock",
        "fill": "$sidebar/status/indicator/lock",
        "icon": "$cool-gray/700",
    },
    {
        "id": "danger",
        "label": "Danger",
        "fill": "$sidebar/status/indicator/danger",
    },
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def status_dot(variant_id: str, fill: str, children: list[dict] | None = None) -> dict:
    node = {
        "type": "frame",
        "id": f"atom-status-dot-{variant_id}-dot",
        "name": "dot",
        "x": STATUS_INSET,
        "y": STATUS_INSET,
        "width": STATUS_DOT,
        "height": STATUS_DOT,
        "cornerRadius": STATUS_DOT / 2,
        "layout": "none",
        "clipContent": False,
        "fill": solid(fill),
        "children": children or [],
    }
    return node


def status_lock_icon(fill: str) -> dict:
    return {
        "type": "frame",
        "id": "atom-status-dot-lock-keyhole",
        "name": "lock-keyhole",
        "x": STATUS_ICON_X,
        "y": STATUS_ICON_Y,
        "width": STATUS_ICON_SIZE,
        "height": STATUS_ICON_SIZE,
        "layout": "none",
        "clipContent": False,
        "children": [
            {
                "type": "path",
                "id": "atom-status-dot-lock-icon",
                "name": "icon",
                "d": STATUS_LOCK_D,
                "x": STATUS_LOCK_X,
                "y": STATUS_LOCK_Y,
                "width": STATUS_LOCK_W,
                "height": STATUS_LOCK_H,
                "fillRule": "evenodd",
                "fill": solid(fill),
            }
        ],
    }


def status_frame(variant: dict, y: float) -> dict:
    icon = [status_lock_icon(variant["icon"])] if "icon" in variant else None
    return {
        "type": "frame",
        "id": f"atom-status-dot-{variant['id']}",
        "name": f"Status/{variant['label']}",
        "reusable": True,
        "x": 0,
        "y": y,
        "width": STATUS_SIZE,
        "height": STATUS_SIZE,
        "layout": "none",
        "clipContent": False,
        "children": [status_dot(variant["id"], variant["fill"], icon)],
    }


def status_frames() -> list[dict]:
    step = STATUS_SIZE + STATUS_GAP
    return [status_frame(variant, index * step) for index, variant in enumerate(STATUS_VARIANTS)]


def status_page() -> dict:
    return {
        "id": "page-atom-status",
        "name": "Atoms · Status",
        "children": status_frames(),
    }
