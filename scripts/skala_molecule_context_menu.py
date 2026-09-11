"""Context menu molecules — Base / WithIcon / Search / Semantic / PageSize / Badges.

Shell from the Base SVG: 320×316, rx 8, pad 8, shadow dy=6 blur=8 at 8%.
Item 4 is Selected (fill + Bondi stroke). Trigger offset is 4px — that
lives on Pagination/PageSize/Open, not on the menu itself. Search uses
Input tokens at menu width (Input/Icon is a fixed 236). Badges trail Chip.
"""

from __future__ import annotations

MENU_W = 320.0
MENU_PAD = 8.0
MENU_RADIUS = 8.0
MENU_SHADOW = {
    "type": "shadow",
    "offsetX": 0,
    "offsetY": 6,
    "blur": 8,
    "spread": 0,
    "color": "#00000014",
}
SAMPLE_COUNT = 10
SELECTED_INDEX = 3
SEARCH_H = 32.0
SEARCH_RADIUS = 8.0
SEARCH_PAD = [8.0, 12.0]
SEARCH_GAP = 8.0
SEARCH_ICON = 16.0
SEARCH_PLACEHOLDER = "Поиск"
PAGE_SIZE_OPTIONS = ("10", "20", "50", "100")
PAGE_SIZE_MENU_W = 160.0
TRIGGER_OFFSET = 4.0
MOL_SET_GAP = 40.0


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def instance(node_id: str, target: str, descendants: dict | None = None) -> dict:
    node: dict = {"type": "ref", "id": node_id, "ref": target}
    if descendants:
        node["descendants"] = descendants
    return node


def text_override(state_id: str, content: str) -> dict:
    return {f"atom-context-menu-item-text-{state_id}-label": {"content": content}}


def icon_override(state_id: str, content: str | None = None) -> dict | None:
    if content is None:
        return None
    return {f"atom-context-menu-item-icon-{state_id}-label": {"content": content}}


def chip_item_ref(menu_id: str, index: int, *, selected: bool) -> dict:
    state_id = "selected" if selected else "default"
    return instance(
        f"{menu_id}-item-{index}",
        f"atom-context-menu-item-chip-{state_id}",
    )


def text_item_ref(menu_id: str, index: int, *, selected: bool, content: str | None = None) -> dict:
    state_id = "selected" if selected else "default"
    target = f"atom-context-menu-item-text-{state_id}"
    descendants = text_override(state_id, content) if content is not None else None
    return instance(f"{menu_id}-item-{index}", target, descendants)


def icon_item_ref(
    menu_id: str,
    index: int,
    *,
    state_id: str,
    content: str | None = None,
) -> dict:
    target = f"atom-context-menu-item-icon-{state_id}"
    return instance(
        f"{menu_id}-item-{index}",
        target,
        icon_override(state_id, content),
    )


def menu_shell(
    root_id: str,
    name: str,
    children: list[dict],
    *,
    width: float = MENU_W,
    x: float = 0.0,
    y: float = 0.0,
    gap: float = 0.0,
) -> dict:
    return {
        "type": "frame",
        "id": root_id,
        "name": name,
        "reusable": True,
        "x": x,
        "y": y,
        "width": width,
        "height": "fit_content",
        "layout": "vertical",
        "gap": gap,
        "padding": MENU_PAD,
        "cornerRadius": MENU_RADIUS,
        "clipContent": True,
        "fill": solid("$context-menu/background/default"),
        "effects": [dict(MENU_SHADOW)],
        "children": children,
    }


def list_items(menu_id: str, *, with_icon: bool = False, with_chip: bool = False) -> list[dict]:
    items = []
    for index in range(SAMPLE_COUNT):
        selected = index == SELECTED_INDEX
        if with_chip:
            items.append(chip_item_ref(menu_id, index, selected=selected))
        elif with_icon:
            items.append(
                icon_item_ref(
                    menu_id,
                    index,
                    state_id="selected" if selected else "default",
                )
            )
        else:
            items.append(text_item_ref(menu_id, index, selected=selected))
    return items


def item_list(menu_id: str, items: list[dict]) -> dict:
    return {
        "type": "frame",
        "id": f"{menu_id}-list",
        "name": "list",
        "width": "fill_container",
        "height": "fit_content",
        "layout": "vertical",
        "gap": 0,
        "clipContent": True,
        "children": items,
    }


def search_field(menu_id: str) -> dict:
    # Input/Icon is 236; the menu inner is 304. Inline Input chrome so the
    # field fills the shell. Not a new atom.
    return {
        "type": "frame",
        "id": f"{menu_id}-search",
        "name": "search",
        "width": "fill_container",
        "height": SEARCH_H,
        "layout": "horizontal",
        "gap": SEARCH_GAP,
        "padding": SEARCH_PAD,
        "alignItems": "center",
        "cornerRadius": SEARCH_RADIUS,
        "clipContent": False,
        "fill": solid("$input/background/default"),
        "stroke": {
            "thickness": 1,
            "fill": solid("$input/border/default"),
        },
        "children": [
            {
                "type": "icon_font",
                "id": f"{menu_id}-search-icon",
                "name": "icon",
                "iconFontName": "search",
                "iconFontFamily": "lucide",
                "width": SEARCH_ICON,
                "height": SEARCH_ICON,
                "fill": solid("$input/icon/default"),
            },
            {
                "type": "text",
                "id": f"{menu_id}-search-label",
                "name": "label",
                "content": SEARCH_PLACEHOLDER,
                "fontFamily": "Roboto",
                "fontSize": 14,
                "fontWeight": 400,
                "lineHeight": round(16 / 14, 4),
                "textAlign": "left",
                "fill": solid("$input/text/default"),
            },
        ],
    }


def search_divider(menu_id: str) -> dict:
    return {
        "type": "frame",
        "id": f"{menu_id}-divider",
        "name": "divider",
        "width": "fill_container",
        "height": 1.0,
        "layout": "none",
        "clipContent": False,
        "fill": solid("$dropdown/border/default"),
        "children": [],
    }


def context_menu_base() -> dict:
    root_id = "molecule-context-menu-base"
    return menu_shell(root_id, "ContextMenu/Base", list_items(root_id, with_icon=False))


def context_menu_icon() -> dict:
    root_id = "molecule-context-menu-icon"
    return menu_shell(root_id, "ContextMenu/WithIcon", list_items(root_id, with_icon=True))


def context_menu_search() -> dict:
    root_id = "molecule-context-menu-search"
    children = [
        search_field(root_id),
        search_divider(root_id),
        item_list(root_id, list_items(root_id, with_icon=False)),
    ]
    return menu_shell(root_id, "ContextMenu/Search", children, gap=8.0)


def context_menu_search_icon() -> dict:
    root_id = "molecule-context-menu-search-icon"
    children = [
        search_field(root_id),
        search_divider(root_id),
        item_list(root_id, list_items(root_id, with_icon=True)),
    ]
    return menu_shell(root_id, "ContextMenu/SearchIcon", children, gap=8.0)


def context_menu_semantic() -> dict:
    root_id = "molecule-context-menu-semantic"
    children = [
        icon_item_ref(root_id, 0, state_id="danger", content="Danger"),
        icon_item_ref(root_id, 1, state_id="info", content="Info"),
        icon_item_ref(root_id, 2, state_id="success", content="Success"),
    ]
    return menu_shell(root_id, "ContextMenu/Semantic", children)


def context_menu_badges() -> dict:
    root_id = "molecule-context-menu-badges"
    return menu_shell(
        root_id,
        "ContextMenu/Badges",
        list_items(root_id, with_chip=True),
    )


def context_menu_search_badges() -> dict:
    root_id = "molecule-context-menu-search-badges"
    children = [
        search_field(root_id),
        search_divider(root_id),
        item_list(root_id, list_items(root_id, with_chip=True)),
    ]
    return menu_shell(root_id, "ContextMenu/SearchBadges", children, gap=8.0)


def context_menu_pagesize() -> dict:
    root_id = "molecule-context-menu-pagesize"
    children = []
    for index, value in enumerate(PAGE_SIZE_OPTIONS):
        children.append(
            text_item_ref(
                root_id,
                index,
                selected=index == 0,
                content=value,
            )
        )
    return menu_shell(
        root_id,
        "ContextMenu/PageSize",
        children,
        width=PAGE_SIZE_MENU_W,
    )


def pagination_pagesize_open() -> dict:
    # Catalog sample: closed trigger + 4px + PageSize menu. Live popover
    # on click is editor chrome, not this master.
    return {
        "type": "frame",
        "id": "molecule-pagination-pagesize-open",
        "name": "Pagination/PageSize/Open",
        "reusable": True,
        "x": 0,
        "y": 0,
        "width": "fit_content",
        "height": "fit_content",
        "layout": "vertical",
        "gap": TRIGGER_OFFSET,
        "alignItems": "start",
        "clipContent": False,
        "children": [
            instance(
                "molecule-pagination-pagesize-open-trigger",
                "atom-pagination-pagesize-default",
                {"atom-pagination-pagesize-default-label": {"content": "10"}},
            ),
            instance(
                "molecule-pagination-pagesize-open-menu",
                "molecule-context-menu-pagesize",
            ),
        ],
    }


def context_menu_page() -> dict:
    frames = [
        context_menu_base(),
        context_menu_icon(),
        context_menu_badges(),
        context_menu_search(),
        context_menu_search_icon(),
        context_menu_search_badges(),
        context_menu_semantic(),
        context_menu_pagesize(),
    ]
    for index, frame in enumerate(frames):
        frame["x"] = index * (MENU_W + MOL_SET_GAP)
        frame["y"] = 0
    return {
        "id": "page-molecule-context-menu",
        "name": "Molecules · ContextMenu",
        "children": frames,
    }
