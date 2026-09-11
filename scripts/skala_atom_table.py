"""Table icons, header cells, and body cells.

Geometry from three Figma SVGs (296×188 icons, 1762×52 header strip,
7245×323 body matrix). Trust SVG, not the inspector. Checkbox header
is 56 (centred 20×20); the 262 CheckboxTextSortFilter cell is checkbox
+ label + sort/filter. Body matches. Source:
`design/atoms/table-icon.source.svg`, `design/atoms/table-header.source.svg`.
Rebuild: `python3 scripts/merge_skala_atoms.py`.
"""

from __future__ import annotations

ICON_12 = 12.0
ICON_16 = 16.0
ICON_SET_GAP = 20.0

HEADER_H = 52.0
HEADER_TEXT_W = 262.0
HEADER_ICON_W = 44.0
HEADER_CHECKBOX_W = 56.0
HEADER_SORTFILTER_W = 82.0
HEADER_PAD_X = 16.0
HEADER_ICON_GAP = 4.0
HEADER_LABEL = "Расположение"

CELL_H = 44.0
CELL_TEXT_W = 200.0
CELL_ICON_W = 44.0
CELL_CHECKBOX_W = 56.0
CELL_PAD_X = 16.0
CHECKBOX_UNCHECKED = "atom-checkbox-unchecked-default-icon"
CELL_LABEL = "Label"
CELL_LINK = "Link"

FONT = 14
HEADER_WEIGHT = 500
CELL_WEIGHT = 400
LINE_HEIGHT = round(16 / 14, 4)

ROW_GAP = 20.0

# SVG1: lock / filter / sort / kebab. Header+body add the 16px chrome.
ICONS = (
    ("lock-default", "Lock/Default", "lock", "$table/icon/default", ICON_12),
    ("lock-active", "Lock/Active", "lock", "$table/icon/brand", ICON_12),
    ("lock-disabled", "Lock/Disabled", "lock", "$cool-gray/200", ICON_12),
    ("filter-default", "Filter/Default", "funnel", "$table/icon/default", ICON_12),
    ("filter-active", "Filter/Active", "funnel", "$table/icon/brand", ICON_12),
    ("sort-default", "Sort/Default", "arrow-up-down", "$table/icon/default", ICON_12),
    ("sort-desc", "Sort/Desc", "arrow-down", "$table/icon/brand", ICON_12),
    ("sort-asc", "Sort/Asc", "arrow-up", "$table/icon/brand", ICON_12),
    ("kebab", "Kebab", "ellipsis-vertical", "$table/icon/default", ICON_12),
    ("grip", "Grip", "grip-vertical", "$table/icon/default", ICON_12),
    ("chevron", "Chevron", "chevron-right", "$table/icon/default", ICON_12),
    ("settings", "Settings", "settings", "$table/icon/default", ICON_16),
    ("columns", "Columns", "rows-3", "$table/icon/default", ICON_16),
    ("hide", "Hide", "x", "$table/icon/default", ICON_16),
    ("info", "Info", "info", "$table/icon/info", ICON_16),
)

CELL_STATES = (
    ("default", "Default", "$table/row/background/default"),
    ("alternative", "Alternative", "$table/row/background/alternative"),
)


def solid(color: str) -> list[dict]:
    return [{"type": "solid", "color": color}]


def lucide(node_id: str, glyph: str, fill: str, size: float) -> dict:
    return {
        "type": "icon_font",
        "id": node_id,
        "name": "icon",
        "iconFontName": glyph,
        "iconFontFamily": "lucide",
        "width": size,
        "height": size,
        "fill": solid(fill),
    }


def instance(node_id: str, target: str, descendants: dict | None = None) -> dict:
    node: dict = {"type": "ref", "id": node_id, "ref": target}
    if descendants:
        node["descendants"] = descendants
    return node


def icon_master_id(icon_id: str) -> str:
    return f"atom-table-icon-{icon_id}"


def label_node(node_id: str, content: str, weight: int, fill: str) -> dict:
    return {
        "type": "text",
        "id": node_id,
        "name": "label",
        "content": content,
        "fontFamily": "Roboto",
        "fontSize": FONT,
        "fontWeight": weight,
        "lineHeight": LINE_HEIGHT,
        "textAlign": "left",
        "fill": solid(fill),
    }


def rule(root_id: str, width: float, y: float, token: str) -> dict:
    return {
        "type": "rectangle",
        "id": f"{root_id}-rule",
        "name": "rule",
        "x": 0,
        "y": y,
        "width": width,
        "height": 1,
        "fill": solid(token),
    }


def icon_item(icon_id: str, name: str, glyph: str, fill: str, size: float, x: float, y: float) -> dict:
    root_id = icon_master_id(icon_id)
    return {
        "type": "frame",
        "id": root_id,
        "name": f"Table/Icon/{name}",
        "reusable": True,
        "x": x,
        "y": y,
        "width": size,
        "height": size,
        "layout": "horizontal",
        "justifyContent": "center",
        "alignItems": "center",
        "clipContent": False,
        "children": [lucide(f"{root_id}-icon", glyph, fill, size)],
    }


def cell_frame(
    root_id: str,
    name: str,
    x: float,
    y: float,
    width: float,
    height: float,
    fill: str,
    border: str,
    content_children: list[dict],
    pad_x: float,
    justify: str,
    gap: float = 0.0,
) -> dict:
    content: dict = {
        "type": "frame",
        "id": f"{root_id}-content",
        "name": "content",
        "x": 0,
        "y": 0,
        "width": width,
        "height": height,
        "layout": "horizontal",
        "padding": [0, pad_x],
        "justifyContent": justify,
        "alignItems": "center",
        "clipContent": False,
        "children": content_children,
    }
    if gap:
        content["gap"] = gap
    return {
        "type": "frame",
        "id": root_id,
        "name": name,
        "reusable": True,
        "x": x,
        "y": y,
        "width": width,
        "height": height,
        "layout": "none",
        "clipContent": False,
        "fill": solid(fill),
        "children": [content, rule(root_id, width, height - 1, border)],
    }


def icon_cluster(root_id: str, icon_ids: tuple[str, ...]) -> dict:
    return {
        "type": "frame",
        "id": f"{root_id}-icons",
        "name": "icons",
        "width": "fit_content",
        "height": "fit_content",
        "layout": "horizontal",
        "gap": HEADER_ICON_GAP,
        "alignItems": "center",
        "clipContent": False,
        "children": [
            instance(f"{root_id}-{icon_id}", icon_master_id(icon_id))
            for icon_id in icon_ids
        ],
    }


def header_cell(
    kind: str,
    label: str,
    x: float,
    y: float,
    width: float,
    children: list[dict],
    pad_x: float,
    justify: str,
    gap: float = 0.0,
) -> dict:
    return cell_frame(
        f"atom-table-header-{kind}",
        f"Table/Header/{label}",
        x,
        y,
        width,
        HEADER_H,
        "$table/header/background/default",
        "$table/header/border/default",
        children,
        pad_x,
        justify,
        gap,
    )


def body_cell(
    kind: str,
    label: str,
    state_id: str,
    state_label: str,
    fill: str,
    x: float,
    y: float,
    width: float,
    children: list[dict],
    pad_x: float,
    justify: str,
    gap: float = 0.0,
) -> dict:
    return cell_frame(
        f"atom-table-cell-{kind}-{state_id}",
        f"Table/Cell/{label}/{state_label}",
        x,
        y,
        width,
        CELL_H,
        fill,
        "$table/row/border/default",
        children,
        pad_x,
        justify,
        gap,
    )


def pack(builders, y: float, gap: float = ROW_GAP) -> tuple[list[dict], float]:
    x = 0.0
    nodes = []
    height = 0.0
    for build in builders:
        node = build(x, y)
        nodes.append(node)
        width = float(node["width"])
        height = max(height, float(node["height"]))
        x += width + gap
    return nodes, height


def header_builders() -> list:
    def text_sort_filter(x, y):
        root = "atom-table-header-text-sort-filter"
        return header_cell(
            "text-sort-filter",
            "TextSortFilter",
            x,
            y,
            HEADER_TEXT_W,
            [
                label_node(f"{root}-label", HEADER_LABEL, HEADER_WEIGHT, "$table/text/primary"),
                icon_cluster(root, ("sort-default", "filter-default")),
            ],
            HEADER_PAD_X,
            "space_between",
        )

    def text(x, y):
        root = "atom-table-header-text"
        return header_cell(
            "text",
            "Text",
            x,
            y,
            HEADER_TEXT_W,
            [label_node(f"{root}-label", HEADER_LABEL, HEADER_WEIGHT, "$table/text/primary")],
            HEADER_PAD_X,
            "start",
        )

    def text_info_sort_filter(x, y):
        root = "atom-table-header-text-info-sort-filter"
        leading = {
            "type": "frame",
            "id": f"{root}-leading",
            "name": "leading",
            "width": "fit_content",
            "height": "fit_content",
            "layout": "horizontal",
            "gap": 8,
            "alignItems": "center",
            "clipContent": False,
            "children": [
                label_node(f"{root}-label", HEADER_LABEL, HEADER_WEIGHT, "$table/text/primary"),
                instance(f"{root}-info", icon_master_id("info")),
            ],
        }
        return header_cell(
            "text-info-sort-filter",
            "TextInfoSortFilter",
            x,
            y,
            HEADER_TEXT_W,
            [leading, icon_cluster(root, ("sort-default", "filter-default"))],
            HEADER_PAD_X,
            "space_between",
        )

    def checkbox_text_sort_filter(x, y):
        root = "atom-table-header-checkbox-text-sort-filter"
        leading = {
            "type": "frame",
            "id": f"{root}-leading",
            "name": "leading",
            "width": "fit_content",
            "height": "fit_content",
            "layout": "horizontal",
            "gap": 8,
            "alignItems": "center",
            "clipContent": False,
            "children": [
                instance(f"{root}-checkbox", CHECKBOX_UNCHECKED),
                label_node(f"{root}-label", HEADER_LABEL, HEADER_WEIGHT, "$table/text/primary"),
            ],
        }
        return header_cell(
            "checkbox-text-sort-filter",
            "CheckboxTextSortFilter",
            x,
            y,
            HEADER_TEXT_W,
            [leading, icon_cluster(root, ("sort-default", "filter-default"))],
            HEADER_PAD_X,
            "space_between",
        )

    def checkbox(x, y):
        return header_cell(
            "checkbox",
            "Checkbox",
            x,
            y,
            HEADER_CHECKBOX_W,
            [instance("atom-table-header-checkbox-box", CHECKBOX_UNCHECKED)],
            0,
            "center",
        )

    def empty(x, y):
        return header_cell("empty", "Empty", x, y, HEADER_ICON_W, [], 0, "center")

    def icon_header(kind, name):
        def build(x, y):
            return header_cell(
                kind,
                name,
                x,
                y,
                HEADER_ICON_W,
                [instance(f"atom-table-header-{kind}-icon", icon_master_id(kind))],
                0,
                "center",
            )

        return build

    def sort_filter(x, y):
        root = "atom-table-header-sort-filter"
        return header_cell(
            "sort-filter",
            "SortFilter",
            x,
            y,
            HEADER_SORTFILTER_W,
            [
                instance(f"{root}-grip", icon_master_id("grip")),
                instance(f"{root}-sort", icon_master_id("sort-default")),
                instance(f"{root}-filter", icon_master_id("filter-default")),
            ],
            HEADER_PAD_X,
            "start",
            8,
        )

    return [
        text_sort_filter,
        checkbox_text_sort_filter,
        text,
        text_info_sort_filter,
        empty,
        checkbox,
        icon_header("settings", "Settings"),
        icon_header("columns", "Columns"),
        icon_header("hide", "Hide"),
        icon_header("info", "Info"),
        icon_header("chevron", "Chevron"),
        sort_filter,
    ]


def body_builders(state_id: str, state_label: str, fill: str) -> list:
    def text(x, y):
        root = f"atom-table-cell-text-{state_id}"
        return body_cell(
            "text",
            "Text",
            state_id,
            state_label,
            fill,
            x,
            y,
            CELL_TEXT_W,
            [label_node(f"{root}-label", CELL_LABEL, CELL_WEIGHT, "$table/text/primary")],
            CELL_PAD_X,
            "start",
        )

    def link(x, y):
        root = f"atom-table-cell-link-{state_id}"
        return body_cell(
            "link",
            "Link",
            state_id,
            state_label,
            fill,
            x,
            y,
            CELL_TEXT_W,
            [label_node(f"{root}-label", CELL_LINK, CELL_WEIGHT, "$table/text/link")],
            CELL_PAD_X,
            "start",
        )

    def text_icon(x, y):
        root = f"atom-table-cell-text-icon-{state_id}"
        return body_cell(
            "text-icon",
            "TextIcon",
            state_id,
            state_label,
            fill,
            x,
            y,
            CELL_TEXT_W,
            [
                label_node(f"{root}-label", CELL_LABEL, CELL_WEIGHT, "$table/text/primary"),
                instance(f"{root}-icon", icon_master_id("info")),
            ],
            CELL_PAD_X,
            "start",
            8,
        )

    def badge(x, y):
        root = f"atom-table-cell-badge-{state_id}"
        return body_cell(
            "badge",
            "Badge",
            state_id,
            state_label,
            fill,
            x,
            y,
            CELL_TEXT_W,
            [
                instance(
                    f"{root}-badge",
                    "atom-badge-green-filled-text",
                    {"atom-badge-green-filled-text-label": {"content": "VALUE"}},
                )
            ],
            CELL_PAD_X,
            "start",
        )

    def empty(x, y):
        return body_cell(
            "empty",
            "Empty",
            state_id,
            state_label,
            fill,
            x,
            y,
            CELL_ICON_W,
            [],
            0,
            "center",
        )

    def checkbox(x, y):
        return body_cell(
            "checkbox",
            "Checkbox",
            state_id,
            state_label,
            fill,
            x,
            y,
            CELL_CHECKBOX_W,
            [
                instance(
                    f"atom-table-cell-checkbox-{state_id}-box",
                    CHECKBOX_UNCHECKED,
                )
            ],
            0,
            "center",
        )

    def icon_body(kind, name):
        def build(x, y):
            return body_cell(
                kind,
                name,
                state_id,
                state_label,
                fill,
                x,
                y,
                CELL_ICON_W,
                [
                    instance(
                        f"atom-table-cell-{kind}-{state_id}-icon",
                        icon_master_id(kind),
                    )
                ],
                0,
                "center",
            )

        return build

    return [
        text,
        link,
        text_icon,
        badge,
        empty,
        checkbox,
        icon_body("kebab", "Kebab"),
        icon_body("chevron", "Chevron"),
        icon_body("grip", "Grip"),
    ]


def table_page() -> dict:
    children: list[dict] = []
    x = 0.0
    for icon_id, name, glyph, fill, size in ICONS:
        children.append(icon_item(icon_id, name, glyph, fill, size, x, 0))
        x += size + ICON_SET_GAP
    y = ICON_16 + ROW_GAP
    headers, header_h = pack(header_builders(), y)
    children.extend(headers)
    y += header_h + ROW_GAP
    for state_id, state_label, fill in CELL_STATES:
        bodies, body_h = pack(body_builders(state_id, state_label, fill), y)
        children.extend(bodies)
        y += body_h + ROW_GAP
    return {
        "id": "page-atom-table",
        "name": "Atoms · Table",
        "children": children,
    }
