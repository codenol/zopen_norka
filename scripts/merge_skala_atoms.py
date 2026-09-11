#!/usr/bin/env python3
"""Merge Skala atom, molecule, organism, and template pages into the kit."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / "design" / "skala-spectrum.lib.op"
ATOMS_DIR = ROOT / "design" / "atoms"
MOLECULES_DIR = ROOT / "design" / "molecules"
ORGANISMS_DIR = ROOT / "design" / "organisms"
TEMPLATES_DIR = ROOT / "design" / "templates"

# Lucide slot in the 40×40 MenuButton. 20px glyph, centered.
BUTTON_ICON = 20.0
BUTTON_ICON_INSET = (40.0 - BUTTON_ICON) / 2
BUTTON_SAMPLE_ICON = "bell"

# `token_hex` used to treat alpha 0 as opaque (`0 or 1`). Restore the
# slots that Figma aliases to `white/0-t`.
TRANSPARENT_WHITE = "#FFFFFF00"
_BTN_SENTIMENTS = (
    "accent",
    "danger",
    "warning",
    "success",
    "info",
    "secondary",
)
TRANSPARENT_VARS = (
    "white/0-t",
    "sidebar/menubutton/background/default",
    "sidebar/menubutton/border/default",
    "sidebar/menubutton/border/hover",
    "sidebar/menubutton/border/active_hover",
    "sidebar/menuitem/background/default",
    "sidebar/menuitem/border/default",
    "sidebar/menuitem/border/hover",
    "sidebar/menuitem/border/active_hover",
    *(
        f"button/outline/{sentiment}/background/{state}"
        for sentiment in _BTN_SENTIMENTS
        for state in ("default", "disabled", "focus")
    ),
    *(
        f"button/ghost/{sentiment}/background/{state}"
        for sentiment in _BTN_SENTIMENTS
        for state in ("default", "disabled", "focus")
    ),
    *(
        f"button/ghost/{sentiment}/border/{state}"
        for sentiment in _BTN_SENTIMENTS
        for state in ("default", "hover", "active", "disabled")
    ),
    "pagination/item/number/background/default",
    "pagination/item/arrow/background/default",
    "pagination/item/arrow/background/disabled",
    "pagination/item/ellipsis/background/default",
)

_PATH_TOKEN = r"[MmLlHhVvCcSsQqTtAaZz]|[-+]?(?:\d*\.\d+|\d+)(?:[eE][-+]?\d+)?"
_PATH_ARITY = {
    "M": 2,
    "L": 2,
    "T": 2,
    "H": 1,
    "V": 1,
    "C": 6,
    "S": 4,
    "Q": 4,
    "A": 7,
    "Z": 0,
}

STATES = (
    {
        "id": "default",
        "label": "Default",
        "background": "$sidebar/menubutton/background/default",
        "border": "$sidebar/menubutton/border/default",
        "icon": "$sidebar/menubutton/icon/default",
    },
    {
        "id": "hover",
        "label": "Hover",
        "background": "$sidebar/menubutton/background/hover",
        "border": "$sidebar/menubutton/border/hover",
        "icon": "$sidebar/menubutton/icon/hover",
    },
    {
        "id": "active",
        "label": "Active",
        "background": "$sidebar/menubutton/background/active",
        "border": "$sidebar/menubutton/border/active",
        "icon": "$sidebar/menubutton/icon/active",
    },
    {
        "id": "hover-active",
        "label": "Hover active",
        "background": "$sidebar/menubutton/background/active_hover",
        "border": "$sidebar/menubutton/border/active_hover",
        "icon": "$sidebar/menubutton/icon/active_hover",
    },
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def lucide_icon(
    node_id: str,
    glyph: str,
    fill: str,
    size: float,
    x: float | None = None,
    y: float | None = None,
) -> dict:
    # Omit x/y unless the caller is pinning an absolute slot. An explicit
    # 0,0 is Position::Absolute in jian layout, so the glyph skips the
    # parent's alignItems:center (MenuItem text centered, icon at the top).
    node = {
        "type": "icon_font",
        "id": node_id,
        "name": "icon",
        "iconFontName": glyph,
        "iconFontFamily": "lucide",
        "width": size,
        "height": size,
        "fill": solid(fill),
    }
    if x is not None:
        node["x"] = x
    if y is not None:
        node["y"] = y
    return node


def icon_node(state_id: str, fill: str) -> dict:
    return lucide_icon(
        f"atom-menu-button-{state_id}-icon",
        BUTTON_SAMPLE_ICON,
        fill,
        BUTTON_ICON,
        BUTTON_ICON_INSET,
        BUTTON_ICON_INSET,
    )


def menu_button(state: dict, x: float, y: float) -> dict:
    children = [icon_node(state["id"], state["icon"])]
    # CanvasKit `align: inside` on a filled frame punches the fill. Figma's
    # SVG used a 0.5px-inset centre stroke instead.
    if state["id"] == "active":
        children.insert(
            0,
            {
                "type": "rectangle",
                "id": f"atom-menu-button-{state['id']}-border",
                "name": "border",
                "x": 0.5,
                "y": 0.5,
                "width": 39,
                "height": 39,
                "cornerRadius": 7.5,
                "stroke": {
                    "thickness": 1,
                    "fill": solid(state["border"]),
                },
            },
        )
    return {
        "type": "frame",
        "id": f"atom-menu-button-{state['id']}",
        "name": f"MenuButton/{state['label']}",
        "reusable": True,
        "x": x,
        "y": y,
        "width": 40,
        "height": 40,
        "layout": "none",
        "cornerRadius": 8,
        "clipContent": True,
        "fill": solid(state["background"]),
        "children": children,
    }


def menu_button_page() -> dict:
    # Figma set: 40×220, 20px gap, no labels.
    children = [menu_button(state, 0, index * 60) for index, state in enumerate(STATES)]
    return {
        "id": "page-atom-menu-button",
        "name": "Atoms · MenuButton",
        "children": children,
    }


ITEM_W = 168.0
ITEM_H = 34.0
ITEM_WRAP_H = 52.0
ITEM_GAP = 8.0
ITEM_RADIUS = 8.0
ITEM_PAD_X = 12.0
ITEM_PAD_Y = 8.0
ITEM_ICON = 16.0
ITEM_TEXT_W = ITEM_W - ITEM_PAD_X * 2 - ITEM_ICON - ITEM_GAP
ITEM_LABEL = "Обзор"
ITEM_WRAP_LABEL = "Диагностика метрики"
ITEM_WRAP_ICON = "info"
# Body M 14/18. Two-line hug is 8+36+8=52; one-line is 8+18+8=34.
ITEM_LINE_HEIGHT = round(18 / 14, 4)

ITEM_STATES = (
    {
        "id": "default",
        "label": "Default",
        "icon": "eye",
        "background": "$sidebar/menuitem/background/default",
        "border": "$sidebar/menuitem/border/default",
        "icon_fill": "$sidebar/menuitem/icon/default",
        "text": "$sidebar/menuitem/text/default",
    },
    {
        "id": "hover",
        "label": "Hover",
        "icon": "git-fork",
        "background": "$sidebar/menuitem/background/hover",
        "border": "$sidebar/menuitem/border/hover",
        "icon_fill": "$sidebar/menuitem/icon/hover",
        "text": "$sidebar/menuitem/text/hover",
    },
    {
        "id": "active",
        "label": "Active",
        "icon": "git-fork",
        "background": "$sidebar/menuitem/background/active",
        "border": "$sidebar/menuitem/border/active",
        "icon_fill": "$sidebar/menuitem/icon/active",
        "text": "$sidebar/menuitem/text/active",
    },
    {
        "id": "active-hover",
        "label": "Active hover",
        "icon": "git-fork",
        "background": "$sidebar/menuitem/background/active_hover",
        "border": "$sidebar/menuitem/border/active_hover",
        "icon_fill": "$sidebar/menuitem/icon/active_hover",
        "text": "$sidebar/menuitem/text/active_hover",
    },
)


def _fmt_num(value: float) -> str:
    rounded = round(value, 4)
    if rounded == int(rounded):
        return str(int(rounded))
    return f"{rounded:.4f}".rstrip("0").rstrip(".")


def _path_arity(cmd: str) -> int:
    return _PATH_ARITY[cmd.upper()]


def iter_path_commands(d: str):
    tokens = re.findall(_PATH_TOKEN, d)
    i = 0
    implicit = None
    while i < len(tokens):
        token = tokens[i]
        if token.isalpha():
            cmd = token
            i += 1
        elif implicit is None:
            raise ValueError(f"path data starts with a number: {d[:40]}")
        else:
            cmd = implicit
        arity = _path_arity(cmd)
        nums = [float(tokens[j]) for j in range(i, i + arity)]
        i += arity
        yield cmd, nums
        if cmd in "Mm":
            implicit = "L" if cmd == "M" else "l"
        elif cmd not in "Zz":
            implicit = cmd


def translate_path(d: str, dx: float, dy: float) -> str:
    out: list[str] = []
    for cmd, nums in iter_path_commands(d):
        out.append(cmd)
        if not nums:
            continue
        if cmd.isupper():
            if cmd == "H":
                nums = [nums[0] + dx]
            elif cmd == "V":
                nums = [nums[0] + dy]
            elif cmd == "A":
                nums = nums[:5] + [nums[5] + dx, nums[6] + dy]
            else:
                shifted = []
                for index, value in enumerate(nums):
                    shifted.append(value + (dx if index % 2 == 0 else dy))
                nums = shifted
        out.extend(_fmt_num(value) for value in nums)
    return "".join(
        part if part.isalpha() or (index == 0) else f" {part}"
        for index, part in enumerate(out)
        if True
    )


def _cubic_point(
    p0: tuple[float, float],
    p1: tuple[float, float],
    p2: tuple[float, float],
    p3: tuple[float, float],
    t: float,
) -> tuple[float, float]:
    u = 1.0 - t
    x = (
        u * u * u * p0[0]
        + 3 * u * u * t * p1[0]
        + 3 * u * t * t * p2[0]
        + t * t * t * p3[0]
    )
    y = (
        u * u * u * p0[1]
        + 3 * u * u * t * p1[1]
        + 3 * u * t * t * p2[1]
        + t * t * t * p3[1]
    )
    return x, y


def path_bbox(d: str) -> tuple[float, float, float, float]:
    xs: list[float] = []
    ys: list[float] = []
    cx = cy = mx = my = 0.0

    def add(x: float, y: float) -> None:
        xs.append(x)
        ys.append(y)

    for cmd, nums in iter_path_commands(d):
        if cmd == "M":
            cx, cy = nums
            mx, my = cx, cy
            add(cx, cy)
        elif cmd == "L":
            cx, cy = nums
            add(cx, cy)
        elif cmd == "H":
            cx = nums[0]
            add(cx, cy)
        elif cmd == "V":
            cy = nums[0]
            add(cx, cy)
        elif cmd == "C":
            p0 = (cx, cy)
            p1 = (nums[0], nums[1])
            p2 = (nums[2], nums[3])
            p3 = (nums[4], nums[5])
            for step in range(17):
                x, y = _cubic_point(p0, p1, p2, p3, step / 16)
                add(x, y)
            cx, cy = p3
        elif cmd in "Zz":
            cx, cy = mx, my
        else:
            raise ValueError(f"unsupported path command {cmd}")
    return min(xs), min(ys), max(xs) - min(xs), max(ys) - min(ys)


def menu_item_icon_node(state_id: str, glyph: str, fill: str) -> dict:
    return lucide_icon(
        f"atom-menu-item-{state_id}-icon",
        glyph,
        fill,
        ITEM_ICON,
    )


def menu_item_label(state_id: str, fill: str, content: str) -> dict:
    return {
        "type": "text",
        "id": f"atom-menu-item-{state_id}-label",
        "name": "label",
        "content": content,
        "fontFamily": "Roboto",
        "fontSize": 14,
        "fontWeight": 500,
        "lineHeight": ITEM_LINE_HEIGHT,
        "textAlign": "left",
        "width": ITEM_TEXT_W,
        "textGrowth": "fixed-width",
        "fill": solid(fill),
    }


def menu_item(state: dict, x: float, y: float, content: str = ITEM_LABEL) -> dict:
    # Figma menuItem IS the horizontal hug frame (168 × hug, pad 8/12,
    # gap 8). Icon has no x/y so flex + alignItems:center can place it
    # (y=9 one line, y=18 on wrap). Do not nest a second padded row.
    node = {
        "type": "frame",
        "id": f"atom-menu-item-{state['id']}",
        "name": f"MenuItem/{state['label']}",
        "reusable": True,
        "x": x,
        "y": y,
        "width": ITEM_W,
        "height": "fit_content",
        "minHeight": ITEM_H,
        "layout": "horizontal",
        "gap": ITEM_GAP,
        "padding": [ITEM_PAD_Y, ITEM_PAD_X],
        "alignItems": "center",
        "cornerRadius": ITEM_RADIUS,
        "clipContent": False,
        "fill": solid(state["background"]),
        "children": [
            menu_item_icon_node(state["id"], state["icon"], state["icon_fill"]),
            menu_item_label(state["id"], state["text"], content),
        ],
    }
    # Centre stroke on the hug frame — not `align: inside` (punches fill)
    # and not a sibling `fill_container` rect (it collapses under fit_content).
    if state["id"] == "active":
        node["stroke"] = {
            "thickness": 1,
            "fill": solid(state["border"]),
        }
    return node


def menu_item_wrap_sample() -> dict:
    return {
        "type": "ref",
        "id": "atom-menu-item-wrap-sample",
        "name": "MenuItem/Wrap",
        "ref": "atom-menu-item-default",
        "descendants": {
            "atom-menu-item-default-label": {"content": ITEM_WRAP_LABEL},
            "atom-menu-item-default-icon": {"iconFontName": ITEM_WRAP_ICON},
        },
    }


def menu_item_page() -> dict:
    states = [menu_item(state, 0, 0) for state in ITEM_STATES]
    return {
        "id": "page-atom-menu-item",
        "name": "Atoms · MenuItem",
        "children": [
            {
                "type": "frame",
                "id": "atom-menu-item-set",
                "name": "set",
                "layout": "vertical",
                "gap": ITEM_GAP,
                "children": [states[0], menu_item_wrap_sample(), *states[1:]],
            }
        ],
    }


def replace_page(pages: list[dict], page: dict) -> list[dict]:
    out = [existing for existing in pages if existing.get("id") != page["id"]]
    out.append(page)
    return out


def restore_transparent_vars(variables: dict) -> None:
    for name in TRANSPARENT_VARS:
        if name in variables:
            variables[name] = {"type": "color", "value": TRANSPARENT_WHITE}


def themed_color(light: str, dark: str) -> dict:
    return {
        "type": "color",
        "value": [
            {"value": light, "theme": {"Mode": "Light"}},
            {"value": dark, "theme": {"Mode": "Dark"}},
        ],
    }


def ensure_chip_icon_vars(variables: dict) -> None:
    # SVG fills; not in the Figma alias export. Light matches the Chip set.
    variables["chip/icon/default"] = themed_color("#75777B", "#A8AAB0")
    variables["chip/icon/disabled"] = themed_color("#DFE2E6", "#5C5D62")


LOGO_W = 200.0
LOGO_H = 48.0
LOGO_GAP = 16.0
LOGO_STEP = LOGO_H + LOGO_GAP
LOGO_FILL = {
    "#11244D": "$sidebar/logo/text/default",
    "#00BEC8": "$sidebar/logo/accent/default",
}
LOGO_VARIANTS = (
    {"id": "genome-2", "label": "Геном 2.0"},
    {"id": "vision", "label": "Визион"},
    {"id": "spectrum", "label": "Спектр"},
    {"id": "spectrum-s3", "label": "Спектр.S3"},
    {"id": "logo", "label": "Лого"},
    {"id": "spectrum-ai", "label": "Спектр ИИ"},
    {"id": "spectrum-2", "label": "Спектр 2.0"},
)


def parse_svg_paths(svg: str) -> list[dict]:
    paths = []
    for match in re.finditer(r"<path\b([^>]*)/?>", svg):
        attrs = match.group(1)
        data = re.search(r'\bd="([^"]+)"', attrs)
        fill = re.search(r'\bfill="([^"]+)"', attrs)
        if data is None or fill is None:
            continue
        rule = re.search(r'\bfill-rule="([^"]+)"', attrs)
        d = data.group(1)
        x, y, width, height = path_bbox(d)
        paths.append(
            {
                "d": d,
                "fill": fill.group(1).upper(),
                "fill_rule": rule.group(1) if rule else None,
                "x": x,
                "y": y,
                "width": width,
                "height": height,
            }
        )
    return paths


def logo_path_role(path: dict) -> str:
    if path["fill"] == "#00BEC8":
        return "mark" if path["width"] < 20 else "version"
    return "wordmark"


def logo_frames() -> list[dict]:
    svg = (ATOMS_DIR / "logo.source.svg").read_text()
    buckets: list[list[dict]] = [[] for _ in LOGO_VARIANTS]
    for path in parse_svg_paths(svg):
        row = int((path["y"] + path["height"] / 2) // LOGO_STEP)
        if row < 0 or row >= len(LOGO_VARIANTS):
            raise ValueError(
                f"logo path y={path['y']} h={path['height']} mapped to row {row}"
            )
        token = LOGO_FILL.get(path["fill"])
        if token is None:
            raise ValueError(f"unexpected logo fill {path['fill']}")
        origin_y = row * LOGO_STEP
        local_d = translate_path(path["d"], 0, -origin_y)
        x, y, width, height = path_bbox(local_d)
        node = {
            "type": "path",
            "d": local_d,
            "x": round(x, 4),
            "y": round(y, 4),
            "width": round(width, 4),
            "height": round(height, 4),
            "fill": solid(token),
        }
        if path["fill_rule"]:
            node["fillRule"] = path["fill_rule"]
        buckets[row].append((logo_path_role(path), node))
    frames = []
    for index, variant in enumerate(LOGO_VARIANTS):
        role_counts: dict[str, int] = {}
        children = []
        for role, node in buckets[index]:
            role_counts[role] = role_counts.get(role, 0) + 1
            name = role if role_counts[role] == 1 else f"{role}-{role_counts[role]}"
            node = {
                **node,
                "id": f"atom-logo-{variant['id']}-{name}",
                "name": name,
            }
            children.append(node)
        if not children:
            raise ValueError(f"logo variant {variant['label']} has no paths")
        frames.append(
            {
                "type": "frame",
                "id": f"atom-logo-{variant['id']}",
                "name": f"Logo/{variant['label']}",
                "reusable": True,
                "x": 0,
                "y": index * LOGO_STEP,
                "width": LOGO_W,
                "height": LOGO_H,
                "layout": "none",
                "clipContent": False,
                "children": children,
            }
        )
    return frames


def logo_page() -> dict:
    return {
        "id": "page-atom-logo",
        "name": "Atoms · Logo",
        "children": logo_frames(),
    }


STATUS_GAP = 16.0
STATUS_SIZES = (16, 20)
STATUS_FILL = {
    "#56B361": "$indicator/success/background/default",
    "#E19109": "$indicator/warning/background/default",
    "#6CB7F2": "$indicator/info/background/default",
    "#D41F20": "$indicator/critical/background/default",
    "#8D8F95": "$indicator/unavailable/background/default",
}
STATUS_COLUMNS = (
    {"id": "success", "label": "Success"},
    {"id": "warning", "label": "Warning"},
    {"id": "process", "label": "Process"},
    {"id": "error", "label": "Error"},
    {"id": "zero", "label": "Zero"},
)


def status_cell(cx: float, cy: float) -> tuple[int, int, float, float, float]:
    size = 16 if cy < 24 else 20
    origin_y = 0.0 if size == 16 else 32.0
    step = size + STATUS_GAP
    col = int(cx // step)
    return col, size, col * step, origin_y, size


def status_indicator_frames() -> list[dict]:
    svg = (ATOMS_DIR / "status-indicator.source.svg").read_text()
    cells: dict[tuple[int, int], dict] = {}
    for path in parse_svg_paths(svg):
        col, size, origin_x, origin_y, _ = status_cell(
            path["x"] + path["width"] / 2,
            path["y"] + path["height"] / 2,
        )
        if col < 0 or col >= len(STATUS_COLUMNS):
            raise ValueError(f"status path x={path['x']} mapped to column {col}")
        token = STATUS_FILL.get(path["fill"])
        if token is None:
            raise ValueError(f"unexpected status fill {path['fill']}")
        key = (size, col)
        if key in cells:
            raise ValueError(f"duplicate status cell size={size} col={col}")
        local_d = translate_path(path["d"], -origin_x, -origin_y)
        x, y, width, height = path_bbox(local_d)
        node = {
            "type": "path",
            "id": f"atom-status-{STATUS_COLUMNS[col]['id']}-{size}-icon",
            "name": "icon",
            "d": local_d,
            "x": round(x, 4),
            "y": round(y, 4),
            "width": round(width, 4),
            "height": round(height, 4),
            "fill": solid(token),
        }
        if path["fill_rule"]:
            node["fillRule"] = path["fill_rule"]
        cells[key] = {
            "size": size,
            "col": col,
            "origin_x": origin_x,
            "origin_y": origin_y,
            "node": node,
        }
    if len(cells) != len(STATUS_SIZES) * len(STATUS_COLUMNS):
        raise ValueError(f"expected 10 status icons, got {len(cells)}")
    frames = []
    for size in STATUS_SIZES:
        for col, status in enumerate(STATUS_COLUMNS):
            cell = cells[(size, col)]
            frames.append(
                {
                    "type": "frame",
                    "id": f"atom-status-{status['id']}-{size}",
                    "name": f"StatusIndicator/{status['label']} {size}",
                    "reusable": True,
                    "x": cell["origin_x"],
                    "y": cell["origin_y"],
                    "width": float(size),
                    "height": float(size),
                    "layout": "none",
                    "clipContent": False,
                    "children": [cell["node"]],
                }
            )
    return frames


def status_indicator_page() -> dict:
    return {
        "id": "page-atom-status-indicator",
        "name": "Atoms · StatusIndicator",
        "children": status_indicator_frames(),
    }


def atom_pages() -> list[dict]:
    scripts_dir = str(Path(__file__).resolve().parent)
    if scripts_dir not in sys.path:
        sys.path.insert(0, scripts_dir)
    from skala_atom_avatar import avatar_page
    from skala_atom_button import button_page
    from skala_atom_divider import divider_page
    from skala_atom_input import input_page
    from skala_atom_input_icon import input_icon_page
    from skala_atom_pagination import pagination_atom_page
    from skala_atom_context_menu import context_menu_atom_page
    from skala_atom_chip import chip_page
    from skala_atom_badge_basic import badge_basic_page
    from skala_atom_badge import badge_page
    from skala_atom_status import status_page
    from skala_atom_table import table_page
    from skala_atom_checkbox import checkbox_page

    return [
        menu_button_page(),
        menu_item_page(),
        logo_page(),
        status_indicator_page(),
        status_page(),
        avatar_page(),
        divider_page(),
        button_page(),
        input_page(),
        input_icon_page(),
        pagination_atom_page(),
        context_menu_atom_page(),
        chip_page(),
        badge_basic_page(),
        badge_page(),
        table_page(),
        checkbox_page(),
    ]


def molecule_pages() -> list[dict]:
    scripts_dir = str(Path(__file__).resolve().parent)
    if scripts_dir not in sys.path:
        sys.path.insert(0, scripts_dir)
    from skala_molecule_breadcrumbs import breadcrumbs_page
    from skala_molecule_pagination import pagination_page
    from skala_molecule_context_menu import context_menu_page
    from skala_molecule_button_badge import button_badge_page

    return [
        breadcrumbs_page(),
        pagination_page(),
        context_menu_page(),
        button_badge_page(),
    ]


def organism_pages() -> list[dict]:
    scripts_dir = str(Path(__file__).resolve().parent)
    if scripts_dir not in sys.path:
        sys.path.insert(0, scripts_dir)
    from skala_organism_sidebar import sidebar_page

    return [sidebar_page()]


def template_pages() -> list[dict]:
    scripts_dir = str(Path(__file__).resolve().parent)
    if scripts_dir not in sys.path:
        sys.path.insert(0, scripts_dir)
    from skala_template_layout import layout_page

    return [layout_page()]


def merge_into(lib_path: Path = LIB) -> list[dict]:
    ATOMS_DIR.mkdir(parents=True, exist_ok=True)
    MOLECULES_DIR.mkdir(parents=True, exist_ok=True)
    ORGANISMS_DIR.mkdir(parents=True, exist_ok=True)
    TEMPLATES_DIR.mkdir(parents=True, exist_ok=True)
    pages = atom_pages()
    stems = {
        "page-atom-menu-button": "menu-button",
        "page-atom-menu-item": "menu-item",
        "page-atom-logo": "logo",
        "page-atom-status-indicator": "status-indicator",
        "page-atom-status": "status",
        "page-atom-avatar": "avatar",
        "page-atom-divider": "divider",
        "page-atom-button": "button",
        "page-atom-input": "input",
        "page-atom-input-icon": "input-icon",
        "page-atom-pagination": "pagination",
        "page-atom-context-menu": "context-menu",
        "page-atom-chip": "chip",
        "page-atom-badge-basic": "badge-basic",
        "page-atom-badge": "badge",
        "page-atom-table": "table",
        "page-atom-checkbox": "checkbox",
    }
    library = json.loads(lib_path.read_text())
    restore_transparent_vars(library.get("variables") or {})
    ensure_chip_icon_vars(library.get("variables") or {})
    for page in pages:
        stem = stems[page["id"]]
        (ATOMS_DIR / f"{stem}.json").write_text(
            json.dumps(page, ensure_ascii=False, indent=2) + "\n"
        )
        library["pages"] = replace_page(library.get("pages") or [], page)
        print(f"wrote {lib_path} page {page['name']}")
    molecule_stems = {
        "page-molecule-breadcrumbs": "breadcrumbs",
        "page-molecule-pagination": "pagination",
        "page-molecule-context-menu": "context-menu",
        "page-molecule-button-badge": "button-badge",
    }
    for page in molecule_pages():
        stem = molecule_stems[page["id"]]
        (MOLECULES_DIR / f"{stem}.json").write_text(
            json.dumps(page, ensure_ascii=False, indent=2) + "\n"
        )
        library["pages"] = replace_page(library.get("pages") or [], page)
        print(f"wrote {lib_path} page {page['name']}")
        pages.append(page)
    for page in organism_pages():
        (ORGANISMS_DIR / "sidebar.json").write_text(
            json.dumps(page, ensure_ascii=False, indent=2) + "\n"
        )
        library["pages"] = replace_page(library.get("pages") or [], page)
        print(f"wrote {lib_path} page {page['name']}")
        pages.append(page)
    for page in template_pages():
        (TEMPLATES_DIR / "layout.json").write_text(
            json.dumps(page, ensure_ascii=False, indent=2) + "\n"
        )
        library["pages"] = replace_page(library.get("pages") or [], page)
        print(f"wrote {lib_path} page {page['name']}")
        pages.append(page)
    lib_path.write_text(json.dumps(library, ensure_ascii=False, indent=2) + "\n")
    return pages


def main() -> None:
    merge_into(LIB)


if __name__ == "__main__":
    main()
