"""App layout template — sidebar + breadcrumbs + content slot (Frost template)."""

from __future__ import annotations

from skala_molecule_breadcrumbs import BC_H
from skala_organism_sidebar import SIDEBAR_H, SIDEBAR_W

# Figma «Структура и отступы»: 20px top/left, 16px gutter / right / bottom.
# Inner height matches Sidebar/Default (814) so the rail fills the column.
PAD_TOP = 20.0
PAD_LEFT = 20.0
PAD_RIGHT = 16.0
PAD_BOTTOM = 16.0
GAP = 16.0
FRAME_W = 1440.0
FRAME_H = PAD_TOP + SIDEBAR_H + PAD_BOTTOM
SHELL_RADIUS = 16.0
CANVAS = "$layout/background/default"
SLOT_BG = "$container/background/default"
SLOT_BORDER = "$container/border/default"
SLOT_TEXT = "$breadcrumbs/text/secondary"


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def instance(
    node_id: str,
    master_id: str,
    descendants: dict[str, dict] | None = None,
    **props,
) -> dict:
    node: dict = {
        "type": "ref",
        "id": node_id,
        "ref": master_id,
        **props,
    }
    if descendants:
        node["descendants"] = descendants
    return node


def main_container(height: float) -> dict:
    return {
        "type": "frame",
        "id": "tpl-layout-main",
        "name": "Main container",
        "width": "fill_container",
        "height": height,
        "layout": "horizontal",
        "justifyContent": "center",
        "alignItems": "center",
        "cornerRadius": SHELL_RADIUS,
        "clipContent": True,
        "fill": solid(SLOT_BG),
        "stroke": {
            "thickness": 1,
            "fill": solid(SLOT_BORDER),
        },
        "children": [
            {
                "type": "text",
                "id": "tpl-layout-main-label",
                "name": "label",
                "content": "Main container",
                "fontFamily": "Roboto",
                "fontSize": 14,
                "fontWeight": 500,
                "lineHeight": round(18 / 14, 4),
                "fill": solid(SLOT_TEXT),
            }
        ],
    }


def content_column() -> dict:
    main_h = SIDEBAR_H - BC_H - GAP
    return {
        "type": "frame",
        "id": "tpl-layout-content",
        "name": "Content",
        "width": "fill_container",
        "height": SIDEBAR_H,
        "layout": "vertical",
        "gap": GAP,
        "alignItems": "start",
        "children": [
            instance("tpl-layout-breadcrumbs", "molecule-breadcrumbs-3"),
            main_container(main_h),
        ],
    }


def layout_frame() -> dict:
    content_x = PAD_LEFT + SIDEBAR_W + GAP
    content_w = FRAME_W - content_x - PAD_RIGHT
    column = content_column()
    column["x"] = content_x
    column["y"] = PAD_TOP
    column["width"] = content_w
    return {
        "type": "frame",
        "id": "tpl-layout-default",
        "name": "Layout/Default",
        "reusable": True,
        "x": 0,
        "y": 0,
        "width": FRAME_W,
        "height": FRAME_H,
        "layout": "none",
        "clipContent": True,
        "fill": solid(CANVAS),
        "children": [
            instance(
                "tpl-layout-sidebar",
                "org-sidebar-default",
                x=PAD_LEFT,
                y=PAD_TOP,
                width=SIDEBAR_W,
                height=SIDEBAR_H,
            ),
            column,
        ],
    }


def layout_page() -> dict:
    return {
        "id": "page-template-layout",
        "name": "Templates · Layout",
        "children": [layout_frame()],
    }
