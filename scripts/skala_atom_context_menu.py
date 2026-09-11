"""Context menu items — 30px rows, text or leading icon.

Geometry from the Base SVG (352×348 viewBox, menu 320×316, rx 8).
Item pitch 30; selected fill + 1px centre Bondi stroke (path inset 0.5).
Menu pad 8 + item pad 8 puts the label ~16px from the shell. Catalog
column is 304 wide so fill_container items match the menu inner width.
WithIcon spacing (16×16 lucide, gap 8) is inferred from Button Large —
send the WithIcon SVG to lock it. WithChip trails a Chip atom.
"""

from __future__ import annotations

ITEM_H = 30.0
ITEM_RADIUS = 8.0
ITEM_PAD_X = 8.0
ITEM_GAP = 8.0
ITEM_ICON = 16.0
ITEM_FONT = 14
ITEM_WEIGHT = 400
ITEM_LINE_HEIGHT = round(16 / 14, 4)
ITEM_LABEL = "ContextMenuItem"
ITEM_SAMPLE_ICON = "map-pin"
ITEM_SET_GAP = 16.0
CATALOG_W = 304.0

TEXT_STATES = (
    ("default", "Default"),
    ("hover", "Hover"),
    ("selected", "Selected"),
    ("disabled", "Disabled"),
)
ICON_STATES = TEXT_STATES
SEMANTIC = (
    ("danger", "Danger"),
    ("info", "Info"),
    ("success", "Success"),
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def lucide_icon(node_id: str, glyph: str, fill: str) -> dict:
    return {
        "type": "icon_font",
        "id": node_id,
        "name": "icon",
        "iconFontName": glyph,
        "iconFontFamily": "lucide",
        "width": ITEM_ICON,
        "height": ITEM_ICON,
        "fill": solid(fill),
    }


def item_label(root_id: str, state_id: str, content: str) -> dict:
    return {
        "type": "text",
        "id": f"{root_id}-label",
        "name": "label",
        "content": content,
        "fontFamily": "Roboto",
        "fontSize": ITEM_FONT,
        "fontWeight": ITEM_WEIGHT,
        "lineHeight": ITEM_LINE_HEIGHT,
        "textAlign": "left",
        "fill": solid(f"$context-menu/menu-item/text/{state_id}"),
    }


def item_frame(
    root_id: str,
    name: str,
    state_id: str,
    children: list[dict],
    *,
    selected: bool,
) -> dict:
    node = {
        "type": "frame",
        "id": root_id,
        "name": name,
        "reusable": True,
        "x": 0,
        "y": 0,
        "width": "fill_container",
        "height": ITEM_H,
        "layout": "horizontal",
        "gap": ITEM_GAP,
        "padding": [0.0, ITEM_PAD_X],
        "justifyContent": "start",
        "alignItems": "center",
        "cornerRadius": ITEM_RADIUS,
        "clipContent": False,
        "fill": solid(f"$context-menu/menu-item/background/{state_id}"),
        "children": children,
    }
    # SVG selected row is fill + 1px centre Bondi. Hover is fill only
    # (`$context-menu/menu-item/border/hover` is transparent).
    if selected:
        node["stroke"] = {
            "thickness": 1,
            "fill": solid("$context-menu/menu-item/border/selected"),
        }
    return node


def text_item(state_id: str, state_label: str) -> dict:
    root_id = f"atom-context-menu-item-text-{state_id}"
    return item_frame(
        root_id,
        f"ContextMenuItem/Text/{state_label}",
        state_id,
        [item_label(root_id, state_id, ITEM_LABEL)],
        selected=state_id == "selected",
    )


def chip_ref(item_state: str) -> dict:
    chip_state = "disabled" if item_state == "disabled" else "default"
    target = f"atom-chip-{chip_state}"
    return {
        "type": "ref",
        "id": f"atom-context-menu-item-chip-{item_state}-chip",
        "ref": target,
    }


def chip_item(state_id: str, state_label: str) -> dict:
    root_id = f"atom-context-menu-item-chip-{state_id}"
    node = item_frame(
        root_id,
        f"ContextMenuItem/WithChip/{state_label}",
        state_id,
        [item_label(root_id, state_id, ITEM_LABEL), chip_ref(state_id)],
        selected=state_id == "selected",
    )
    node["justifyContent"] = "space_between"
    return node


def icon_item(state_id: str, state_label: str, *, icon_state: str, chrome_state: str) -> dict:
    root_id = f"atom-context-menu-item-icon-{state_id}"
    return item_frame(
        root_id,
        f"ContextMenuItem/WithIcon/{state_label}",
        chrome_state,
        [
            lucide_icon(
                f"{root_id}-icon",
                ITEM_SAMPLE_ICON,
                f"$context-menu/menu-item/icon/{icon_state}",
            ),
            item_label(root_id, chrome_state, ITEM_LABEL),
        ],
        selected=chrome_state == "selected",
    )


def catalog_column(col_id: str, name: str, children: list[dict], x: float) -> dict:
    return {
        "type": "frame",
        "id": col_id,
        "name": name,
        "x": x,
        "y": 0,
        "width": CATALOG_W,
        "layout": "vertical",
        "gap": ITEM_SET_GAP,
        "clipContent": False,
        "children": children,
    }


def context_menu_atom_page() -> dict:
    text_col = catalog_column(
        "atom-context-menu-item-col-text",
        "Text",
        [text_item(state_id, label) for state_id, label in TEXT_STATES],
        0.0,
    )
    icon_children = [
        icon_item(state_id, label, icon_state=state_id, chrome_state=state_id)
        for state_id, label in ICON_STATES
    ]
    icon_children.extend(
        icon_item(state_id, label, icon_state=state_id, chrome_state="default")
        for state_id, label in SEMANTIC
    )
    icon_col = catalog_column(
        "atom-context-menu-item-col-icon",
        "WithIcon",
        icon_children,
        CATALOG_W + 40.0,
    )
    chip_col = catalog_column(
        "atom-context-menu-item-col-chip",
        "WithChip",
        [chip_item(state_id, label) for state_id, label in TEXT_STATES],
        (CATALOG_W + 40.0) * 2,
    )
    return {
        "id": "page-atom-context-menu",
        "name": "Atoms · ContextMenu",
        "children": [text_col, icon_col, chip_col],
    }
