"""Sidebar 2.0 organism — Genome 2.0 Light sample from atom refs."""

from __future__ import annotations

from skala_atom_divider import DIVIDER_H

# Figma Sidebar 2.0 is 251×814, radius 16. Minibar x=1–49 (48 wide);
# Menu is 200×812 Fill at x=50, y=1. Skip blur: fill already 75% white.
# Menu inspector stroke is 1px outside — do not paint it on the column
# (it would double the shell). A 1px rule at x=49 is the minibar join.
# Do not fill Menu with the same token: it composites on the shell.
SIDEBAR_W = 251.0
SIDEBAR_H = 814.0
SIDEBAR_RADIUS = 16.0
MINIBAR_W = 48.0
MENU_W = 200.0
MENU_X = 50.0
INSET = 1.0
INNER_H = SIDEBAR_H - INSET * 2
MENU_GAP = 8.0
MENU_SLOT_PAD_X = 16.0
MENU_ITEM_W = 168.0
MENU_ITEM_H = 34.0
LOGO_H = 48.0
HEADING_H = 28.0
NAV_GAP = 0.0
HEADER_H = LOGO_H + DIVIDER_H
SLOT_H = INNER_H - HEADER_H - DIVIDER_H - MENU_ITEM_H - 3 * MENU_GAP
SHELL_BG = "$sidebar/background/default"
SHELL_BORDER = "$sidebar/border/default"
HEADING_FILL = "$sidebar/text/default"

# Builtin lucide names only (`Icon::from_name`) so raster preview is not
# empty dots before the Iconify catalog loads.
MINIBAR_TOP = ("bell", "layout-grid", "star")
MINIBAR_BOTTOM = ("settings", "moon", "image", "help-circle")

NAV: tuple[tuple[str, ...], ...] = (
    ("item", "overview", "active", "eye", "Обзор"),
    ("item", "objects", "default", "git-fork", "Объекты"),
    ("heading", "settings", "Настройки"),
    ("item", "metrics", "default", "wrench", "Метрики"),
    ("item", "metrics-diag", "default", "info", "Диагностика метрики"),
    ("item", "alert-rules", "default", "bell", "Правила оповещений"),
    ("item", "expr-builder", "default", "code", "Конструктор выражений"),
    ("item", "recipients", "default", "user", "Список получателей"),
    ("item", "mail-groups", "default", "users", "Группы рассылки"),
    ("item", "send-settings", "default", "mail", "Настройки отправки"),
    ("heading", "security", "Безопасность"),
    ("item", "rbac", "default", "layout-grid", "Ролевая модель"),
    ("item", "tokens", "default", "key", "Токены доступа"),
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def instance(
    node_id: str,
    master_id: str,
    descendants: dict[str, dict] | None = None,
) -> dict:
    node: dict = {
        "type": "ref",
        "id": node_id,
        "ref": master_id,
    }
    if descendants:
        node["descendants"] = descendants
    return node


def menu_button_ref(slot_id: str, glyph: str) -> dict:
    return instance(
        f"org-sidebar-minibar-{slot_id}",
        "atom-menu-button-default",
        {"atom-menu-button-default-icon": {"iconFontName": glyph}},
    )


def menu_item_ref(slot_id: str, state: str, glyph: str, label: str) -> dict:
    return instance(
        f"org-sidebar-item-{slot_id}",
        f"atom-menu-item-{state}",
        {
            f"atom-menu-item-{state}-label": {"content": label},
            f"atom-menu-item-{state}-icon": {"iconFontName": glyph},
        },
    )


def heading(slot_id: str, label: str) -> dict:
    return {
        "type": "frame",
        "id": f"org-sidebar-heading-{slot_id}",
        "name": f"Heading/{label}",
        "width": MENU_ITEM_W,
        "height": HEADING_H,
        "layout": "horizontal",
        "alignItems": "center",
        "padding": [6, 12],
        "children": [
            {
                "type": "text",
                "id": f"org-sidebar-heading-{slot_id}-label",
                "name": "label",
                "content": label,
                "fontFamily": "Roboto",
                "fontSize": 12,
                "fontWeight": 500,
                "fill": solid(HEADING_FILL),
            }
        ],
    }


def minibar() -> dict:
    top = [
        instance("org-sidebar-status", "atom-status-dot-lock"),
        *[menu_button_ref(name, name) for name in MINIBAR_TOP],
    ]
    bottom = [
        *[menu_button_ref(name, name) for name in MINIBAR_BOTTOM],
        instance("org-sidebar-avatar", "atom-avatar-default"),
    ]
    return {
        "type": "frame",
        "id": "org-sidebar-minibar",
        "name": "Minibar",
        "x": INSET,
        "y": INSET,
        "width": MINIBAR_W,
        "height": INNER_H,
        "layout": "vertical",
        "justifyContent": "space_between",
        "alignItems": "center",
        "padding": [8, 4],
        "children": [
            {
                "type": "frame",
                "id": "org-sidebar-minibar-top",
                "name": "top-slot",
                "layout": "vertical",
                "alignItems": "center",
                "children": top,
            },
            {
                "type": "frame",
                "id": "org-sidebar-minibar-bottom",
                "name": "bottom-slot",
                "layout": "vertical",
                "alignItems": "center",
                "children": bottom,
            },
        ],
    }


def nav_children() -> list[dict]:
    children: list[dict] = []
    for row in NAV:
        if row[0] == "heading":
            _, slot_id, label = row
            children.append(heading(slot_id, label))
            continue
        _, slot_id, state, glyph, label = row
        children.append(menu_item_ref(slot_id, state, glyph, label))
    return children


def header() -> dict:
    return {
        "type": "frame",
        "id": "org-sidebar-header",
        "name": "Header",
        "width": MENU_W,
        "height": "fit_content",
        "layout": "vertical",
        "gap": 0,
        "children": [
            instance("org-sidebar-logo", "atom-logo-genome-2"),
            instance("org-sidebar-header-divider", "atom-divider-default"),
        ],
    }


def menu_slot() -> dict:
    return {
        "type": "frame",
        "id": "org-sidebar-menu-slot",
        "name": "Menu slot",
        "width": MENU_W,
        "height": SLOT_H,
        "layout": "vertical",
        "gap": NAV_GAP,
        "padding": [0, MENU_SLOT_PAD_X],
        "alignItems": "start",
        "children": nav_children(),
    }


def bottom_block() -> dict:
    return {
        "type": "frame",
        "id": "org-sidebar-bottom",
        "name": "bottom-block",
        "width": MENU_W,
        "height": "fit_content",
        "layout": "vertical",
        "padding": [0, MENU_SLOT_PAD_X],
        "children": [
            menu_item_ref("collapse", "default", "chevron-left", "Свернуть")
        ],
    }


def menu_column() -> dict:
    return {
        "type": "frame",
        "id": "org-sidebar-menu",
        "name": "Menu",
        "x": MENU_X,
        "y": INSET,
        "width": MENU_W,
        "height": INNER_H,
        "layout": "vertical",
        "gap": MENU_GAP,
        "children": [
            header(),
            menu_slot(),
            instance("org-sidebar-footer-divider", "atom-divider-default"),
            bottom_block(),
        ],
    }


def menu_rule() -> dict:
    return {
        "type": "rectangle",
        "id": "org-sidebar-menu-rule",
        "name": "rule",
        "x": INSET + MINIBAR_W,
        "y": INSET,
        "width": 1,
        "height": INNER_H,
        "fill": solid(SHELL_BORDER),
    }


def sidebar_frame() -> dict:
    return {
        "type": "frame",
        "id": "org-sidebar-default",
        "name": "Sidebar/Default",
        "reusable": True,
        "x": 0,
        "y": 0,
        "width": SIDEBAR_W,
        "height": SIDEBAR_H,
        "layout": "none",
        "cornerRadius": SIDEBAR_RADIUS,
        "clipContent": True,
        "fill": solid(SHELL_BG),
        "children": [
            {
                "type": "rectangle",
                "id": "org-sidebar-border",
                "name": "border",
                "x": 0.5,
                "y": 0.5,
                "width": SIDEBAR_W - 1,
                "height": SIDEBAR_H - 1,
                "cornerRadius": SIDEBAR_RADIUS - 0.5,
                "stroke": {
                    "thickness": 1,
                    "fill": solid(SHELL_BORDER),
                },
            },
            minibar(),
            menu_rule(),
            menu_column(),
        ],
    }


def sidebar_page() -> dict:
    return {
        "id": "page-organism-sidebar",
        "name": "Organisms · Sidebar",
        "children": [sidebar_frame()],
    }
