"""Pagination molecule — assembled bar plus PageSize opening the kit menu."""

from __future__ import annotations

# Assembled SVG is 364×32: prev (disabled) + 1 selected + 2 3 4 +
# ellipsis + 45 + next + page-size trigger. Item gap 8. Open PageSize
# is a catalog sample (trigger + 4px + ContextMenu/PageSize).

PAG_GAP = 8.0
PAG_H = 32.0


def instance(node_id: str, target: str, descendants: dict | None = None) -> dict:
    node: dict = {"type": "ref", "id": node_id, "ref": target}
    if descendants:
        node["descendants"] = descendants
    return node


def number_override(state_id: str, content: str) -> dict:
    return {f"atom-pagination-item-number-{state_id}-label": {"content": content}}


def pagination_default() -> dict:
    return {
        "type": "frame",
        "id": "molecule-pagination-default",
        "name": "Pagination/Default",
        "reusable": True,
        "x": 0,
        "y": 0,
        "width": "fit_content",
        "height": PAG_H,
        "layout": "horizontal",
        "gap": PAG_GAP,
        "alignItems": "center",
        "clipContent": False,
        "children": [
            instance(
                "molecule-pagination-default-prev",
                "atom-pagination-item-arrow-prev-disabled",
            ),
            instance(
                "molecule-pagination-default-page-1",
                "atom-pagination-item-number-selected",
                number_override("selected", "1"),
            ),
            instance(
                "molecule-pagination-default-page-2",
                "atom-pagination-item-number-default",
                number_override("default", "2"),
            ),
            instance(
                "molecule-pagination-default-page-3",
                "atom-pagination-item-number-default",
                number_override("default", "3"),
            ),
            instance(
                "molecule-pagination-default-page-4",
                "atom-pagination-item-number-default",
                number_override("default", "4"),
            ),
            instance(
                "molecule-pagination-default-ellipsis",
                "atom-pagination-item-ellipsis-default",
            ),
            instance(
                "molecule-pagination-default-page-last",
                "atom-pagination-item-number-default",
                number_override("default", "45"),
            ),
            instance(
                "molecule-pagination-default-next",
                "atom-pagination-item-arrow-next-default",
            ),
            instance(
                "molecule-pagination-default-pagesize",
                "atom-pagination-pagesize-default",
                {"atom-pagination-pagesize-default-label": {"content": "10"}},
            ),
        ],
    }


def pagination_page() -> dict:
    from skala_molecule_context_menu import pagination_pagesize_open

    opened = pagination_pagesize_open()
    opened["x"] = 0
    opened["y"] = PAG_H + 40.0
    return {
        "id": "page-molecule-pagination",
        "name": "Molecules · Pagination",
        "children": [pagination_default(), opened],
    }
