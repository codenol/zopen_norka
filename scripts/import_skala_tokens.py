#!/usr/bin/env python3
"""Compile Skala Spectrum Figma token JSON into one OpenPencil .lib.op."""

from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
from typing import Any

ROLE_SWATCHES: list[tuple[str, str]] = [
    ("Canvas", "layout/background/default"),
    ("Surface", "card/background/primary"),
    ("Surface muted", "card/background/secondary"),
    ("Border", "card/border/primary"),
    ("Text", "dialog/text/primary"),
    ("Text secondary", "dialog/text/secondary"),
    ("Text tertiary", "dialog/text/tertiary"),
    ("Primary", "button/filled/accent/background/default"),
    ("Primary hover", "button/filled/accent/background/hover"),
    ("Primary active", "button/filled/accent/background/active"),
    ("Primary disabled", "button/filled/accent/background/disabled"),
    ("Java / brand", "sidebar/logo/accent/default"),
    ("Navy wordmark", "sidebar/logo/text/default"),
    ("Focus / Bondi 500", "input/border/focused"),
    ("Error", "input/border/error"),
    ("Table header", "table/header/background/default"),
    ("Chip", "chip/background/default"),
    ("Tooltip", "tooltip/background/default"),
]

PALETTE_STEPS = ("50", "100", "300", "500", "600", "800", "950")
PALETTE_FAMILIES = (
    "cool-gray",
    "warm-gray",
    "bondi-blue",
    "java",
    "nile-blue",
    "tory-blue",
    "cornflower-blue",
    "shamrock",
    "green",
    "yellow",
    "orange",
    "red",
    "rose",
    "violet",
    "white",
    "black",
)


def walk_tokens(obj: Any, path: str = "") -> dict[str, dict[str, Any]]:
    leaves: dict[str, dict[str, Any]] = {}
    if isinstance(obj, dict):
        if "$value" in obj:
            if path:
                leaves[path] = obj
            return leaves
        for key, value in obj.items():
            if key.startswith("$"):
                continue
            child = f"{path}/{key}" if path else key
            leaves.update(walk_tokens(value, child))
    return leaves


def token_hex(token: dict[str, Any]) -> str:
    value = token["$value"]
    if not isinstance(value, dict):
        raise TypeError(f"expected color object, got {type(value).__name__}")
    hex_value = str(value["hex"]).upper()
    if not hex_value.startswith("#"):
        hex_value = f"#{hex_value}"
    if len(hex_value) == 4:
        hex_value = "#" + "".join(ch * 2 for ch in hex_value[1:])
    hex_value = hex_value[:7]
    raw_alpha = value.get("alpha", 1)
    alpha = 1.0 if raw_alpha is None else float(raw_alpha)
    if abs(alpha - 1.0) > 1e-6:
        return f"{hex_value}{int(round(max(0.0, min(1.0, alpha)) * 255)):02X}"
    return hex_value


def themed_color(light_hex: str, dark_hex: str) -> dict[str, Any]:
    if light_hex == dark_hex:
        return {"type": "color", "value": light_hex}
    return {
        "type": "color",
        "value": [
            {"value": light_hex, "theme": {"Mode": "Light"}},
            {"value": dark_hex, "theme": {"Mode": "Dark"}},
        ],
    }


def alias_name(token: dict[str, Any]) -> str | None:
    ext = token.get("$extensions") or {}
    alias = ext.get("com.figma.aliasData") or {}
    name = alias.get("targetVariableName")
    return name if isinstance(name, str) and name else None


def strip_extensions(obj: Any) -> Any:
    if isinstance(obj, dict):
        return {
            key: strip_extensions(value)
            for key, value in obj.items()
            if key != "$extensions"
        }
    if isinstance(obj, list):
        return [strip_extensions(item) for item in obj]
    return obj


def slug(label: str) -> str:
    return "".join(ch.lower() if ch.isalnum() else "-" for ch in label).strip("-")


def swatch_frame(
    index: int, label: str, token_name: str, x: float, y: float
) -> dict[str, Any]:
    swatch_id = f"swatch-{index:02d}-{slug(label)}"
    return {
        "type": "frame",
        "id": swatch_id,
        "name": label,
        "x": x,
        "y": y,
        "width": 220,
        "height": 72,
        "layout": "horizontal",
        "gap": 12,
        "padding": 12,
        "alignItems": "center",
        "cornerRadius": 8,
        "fill": [{"type": "solid", "color": "#FFFFFF"}],
        "stroke": {
            "thickness": 1,
            "fill": [{"type": "solid", "color": "$card/border/primary"}],
        },
        "children": [
            {
                "type": "rectangle",
                "id": f"{swatch_id}-chip",
                "name": "chip",
                "width": 48,
                "height": 48,
                "cornerRadius": 8,
                "fill": [{"type": "solid", "color": f"${token_name}"}],
            },
            {
                "type": "frame",
                "id": f"{swatch_id}-copy",
                "name": "copy",
                "layout": "vertical",
                "gap": 4,
                "width": "fill_container",
                "children": [
                    {
                        "type": "text",
                        "id": f"{swatch_id}-label",
                        "name": "label",
                        "content": label,
                        "fontFamily": "Roboto",
                        "fontSize": 14,
                        "fontWeight": "500",
                        "fill": [{"type": "solid", "color": "$dialog/text/primary"}],
                    },
                    {
                        "type": "text",
                        "id": f"{swatch_id}-token",
                        "name": "token",
                        "content": token_name,
                        "fontFamily": "Roboto",
                        "fontSize": 11,
                        "fontWeight": "400",
                        "fill": [{"type": "solid", "color": "$dialog/text/secondary"}],
                    },
                ],
            },
        ],
    }


def roles_page(variables: dict[str, Any]) -> dict[str, Any]:
    columns = 3
    gutter = 16
    swatches = []
    for index, (label, token_name) in enumerate(ROLE_SWATCHES):
        if token_name not in variables:
            raise SystemExit(f"role swatch token missing: {token_name}")
        col = index % columns
        row = index // columns
        swatches.append(
            swatch_frame(
                index,
                label,
                token_name,
                24 + col * (220 + gutter),
                72 + row * (72 + gutter),
            )
        )
    return {
        "id": "page-color-roles",
        "name": "Color roles",
        "children": [
            {
                "type": "text",
                "id": "color-roles-title",
                "name": "Title",
                "x": 24,
                "y": 24,
                "content": "Skala Spectrum · color roles",
                "fontFamily": "Roboto",
                "fontSize": 18,
                "fontWeight": "600",
                "fill": [{"type": "solid", "color": "$dialog/text/primary"}],
            },
            *swatches,
        ],
    }


def palette_page(palette: dict[str, str]) -> dict[str, Any]:
    children: list[dict[str, Any]] = [
        {
            "type": "text",
            "id": "palette-title",
            "name": "Title",
            "x": 24,
            "y": 24,
            "content": "Skala Spectrum · palette primitives",
            "fontFamily": "Roboto",
            "fontSize": 18,
            "fontWeight": "600",
            "fill": [{"type": "solid", "color": "$dialog/text/primary"}],
        }
    ]
    for family_index, family in enumerate(PALETTE_FAMILIES):
        y = 72 + family_index * 72
        children.append(
            {
                "type": "text",
                "id": f"palette-family-{slug(family)}",
                "name": family,
                "x": 24,
                "y": y + 16,
                "width": 140,
                "content": family,
                "fontFamily": "Roboto",
                "fontSize": 13,
                "fontWeight": "500",
                "fill": [{"type": "solid", "color": "$dialog/text/primary"}],
            }
        )
        names = [f"{family}/{step}" for step in PALETTE_STEPS if f"{family}/{step}" in palette]
        if not names:
            names = [
                name
                for name in sorted(palette)
                if name.startswith(f"{family}/") and "-t" not in name
            ][:7]
        col = 0
        for name in names:
            children.append(
                {
                    "type": "rectangle",
                    "id": f"palette-chip-{slug(name)}",
                    "name": name,
                    "x": 176 + col * 56,
                    "y": y,
                    "width": 48,
                    "height": 48,
                    "cornerRadius": 8,
                    "fill": [{"type": "solid", "color": f"${name}"}],
                }
            )
            col += 1
    return {"id": "page-palette", "name": "Palette", "children": children}


def dimensions_page(numbers: dict[str, float]) -> dict[str, Any]:
    children: list[dict[str, Any]] = [
        {
            "type": "text",
            "id": "dimensions-title",
            "name": "Title",
            "x": 24,
            "y": 24,
            "content": "Skala Spectrum · dimensions",
            "fontFamily": "Roboto",
            "fontSize": 18,
            "fontWeight": "600",
            "fill": [{"type": "solid", "color": "$dialog/text/primary"}],
        }
    ]
    spacing = [(name, value) for name, value in numbers.items() if name.startswith("spacing/")]
    spacing.sort(key=lambda item: (item[1], item[0]))
    y = 72
    for name, value in spacing:
        width = max(8.0, float(value) * 4.0)
        row_id = f"dim-{slug(name)}"
        children.append(
            {
                "type": "frame",
                "id": row_id,
                "name": name,
                "x": 24,
                "y": y,
                "width": 520,
                "height": 28,
                "layout": "horizontal",
                "gap": 12,
                "alignItems": "center",
                "children": [
                    {
                        "type": "text",
                        "id": f"{row_id}-label",
                        "name": "label",
                        "width": 180,
                        "content": f"{name}  {value:g}px",
                        "fontFamily": "Roboto",
                        "fontSize": 12,
                        "fontWeight": "400",
                        "fill": [{"type": "solid", "color": "$dialog/text/secondary"}],
                    },
                    {
                        "type": "rectangle",
                        "id": f"{row_id}-bar",
                        "name": "bar",
                        "width": width,
                        "height": 8,
                        "cornerRadius": 4,
                        "fill": [
                            {
                                "type": "solid",
                                "color": "$button/filled/accent/background/default",
                            }
                        ],
                    },
                ],
            }
        )
        y += 32

    y += 16
    radii = [
        (name, value)
        for name, value in numbers.items()
        if name.startswith("border-radius/")
    ]
    radii.sort(key=lambda item: (item[1], item[0]))
    x = 24
    for name, value in radii:
        radius = min(24.0, float(value))
        chip_id = f"radius-{slug(name)}"
        children.append(
            {
                "type": "frame",
                "id": chip_id,
                "name": name,
                "x": x,
                "y": y,
                "width": 88,
                "height": 88,
                "layout": "vertical",
                "gap": 8,
                "alignItems": "center",
                "children": [
                    {
                        "type": "rectangle",
                        "id": f"{chip_id}-shape",
                        "name": "shape",
                        "width": 48,
                        "height": 48,
                        "cornerRadius": radius,
                        "fill": [{"type": "solid", "color": "$card/background/secondary"}],
                        "stroke": {
                            "thickness": 1,
                            "fill": [{"type": "solid", "color": "$card/border/primary"}],
                        },
                    },
                    {
                        "type": "text",
                        "id": f"{chip_id}-label",
                        "name": "label",
                        "content": f"{name.rsplit('/', 1)[-1]} {value:g}",
                        "fontFamily": "Roboto",
                        "fontSize": 11,
                        "fontWeight": "400",
                        "fill": [{"type": "solid", "color": "$dialog/text/secondary"}],
                    },
                ],
            }
        )
        x += 100
    return {"id": "page-dimensions", "name": "Dimensions", "children": children}


def type_page(strings: dict[str, str]) -> dict[str, Any]:
    children: list[dict[str, Any]] = [
        {
            "type": "text",
            "id": "type-title",
            "name": "Title",
            "x": 24,
            "y": 24,
            "content": "Skala Spectrum · type families",
            "fontFamily": "Roboto",
            "fontSize": 18,
            "fontWeight": "600",
            "fill": [{"type": "solid", "color": "$dialog/text/primary"}],
        }
    ]
    samples = [
        ("FontFamily/Headlines", 22, "700", "Headlines"),
        ("FontFamily/body", 14, "400", "Body 14 / 400 — the product density."),
        ("FontFamily/Mono", 13, "400", "0123456789 tabular / code"),
    ]
    for index, (name, size, weight, sample) in enumerate(samples):
        family = strings.get(name, "Roboto")
        y = 72 + index * 72
        children.append(
            {
                "type": "text",
                "id": f"type-meta-{slug(name)}",
                "name": name,
                "x": 24,
                "y": y,
                "content": f"{name}  {family}",
                "fontFamily": "Roboto",
                "fontSize": 12,
                "fontWeight": "500",
                "fill": [{"type": "solid", "color": "$dialog/text/secondary"}],
            }
        )
        children.append(
            {
                "type": "text",
                "id": f"type-sample-{slug(name)}",
                "name": f"{name} sample",
                "x": 24,
                "y": y + 22,
                "content": sample,
                "fontFamily": family,
                "fontSize": size,
                "fontWeight": weight,
                "fill": [{"type": "solid", "color": "$dialog/text/primary"}],
            }
        )
    return {"id": "page-type", "name": "Typography", "children": children}


def collect_aliases(
    light_leaves: dict[str, dict[str, Any]], dark_leaves: dict[str, dict[str, Any]]
) -> dict[str, dict[str, str]]:
    aliases: dict[str, dict[str, str]] = {}
    for name, token in light_leaves.items():
        light_alias = alias_name(token)
        dark_alias = alias_name(dark_leaves[name])
        entry: dict[str, str] = {}
        if light_alias:
            entry["Light"] = light_alias
        if dark_alias:
            entry["Dark"] = dark_alias
        if entry:
            aliases[name] = entry
    return aliases


def build_library(
    light: dict[str, Any],
    dark: dict[str, Any],
    palette: dict[str, Any],
    dimensions: dict[str, Any],
    typography: dict[str, Any],
) -> tuple[dict[str, Any], dict[str, dict[str, str]]]:
    light_leaves = walk_tokens(light)
    dark_leaves = walk_tokens(dark)
    missing = sorted(set(light_leaves) - set(dark_leaves))
    extra = sorted(set(dark_leaves) - set(light_leaves))
    if missing or extra:
        raise SystemExit(
            f"token path mismatch: missing in dark={missing[:8]} extra in dark={extra[:8]}"
        )

    variables: dict[str, Any] = {}
    for name in sorted(light_leaves):
        variables[name] = themed_color(
            token_hex(light_leaves[name]), token_hex(dark_leaves[name])
        )

    palette_hex = {name: token_hex(token) for name, token in walk_tokens(palette).items()}
    for name, token in light_leaves.items():
        primitive = alias_name(token)
        if primitive and primitive not in palette_hex:
            palette_hex[primitive] = token_hex(token)
    for name, hex_value in palette_hex.items():
        if name in variables:
            raise SystemExit(f"palette name collides with semantic token: {name}")
        variables[name] = {"type": "color", "value": hex_value}

    numbers: dict[str, float] = {}
    for name, token in walk_tokens(dimensions).items():
        if token.get("$type") != "number":
            continue
        value = float(token["$value"])
        numbers[name] = value
        variables[name] = {"type": "number", "value": value}

    strings: dict[str, str] = {}
    for name, token in walk_tokens(typography).items():
        value = str(token["$value"])
        strings[name] = value
        variables[name] = {"type": "string", "value": value}

    aliases = collect_aliases(light_leaves, dark_leaves)
    return {
        "version": "1.0",
        "name": "Skala Spectrum",
        "themes": {"Mode": ["Light", "Dark"]},
        "variables": variables,
        "pages": [
            roles_page(variables),
            palette_page(palette_hex),
            dimensions_page(numbers),
            type_page(strings),
        ],
        "children": [],
    }, aliases


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--light", type=Path, required=True)
    parser.add_argument("--dark", type=Path, required=True)
    parser.add_argument("--palette", type=Path, required=True)
    parser.add_argument("--dimensions", type=Path, required=True)
    parser.add_argument("--typography", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, default=Path("design"))
    args = parser.parse_args()

    light = json.loads(args.light.read_text())
    dark = json.loads(args.dark.read_text())
    palette = json.loads(args.palette.read_text())
    dimensions = json.loads(args.dimensions.read_text())
    typography = json.loads(args.typography.read_text())
    library, aliases = build_library(light, dark, palette, dimensions, typography)

    tokens_dir = args.out_dir / "tokens"
    tokens_dir.mkdir(parents=True, exist_ok=True)
    (tokens_dir / "color.light.json").write_text(
        json.dumps(strip_extensions(light), ensure_ascii=False, indent=2) + "\n"
    )
    (tokens_dir / "color.dark.json").write_text(
        json.dumps(strip_extensions(dark), ensure_ascii=False, indent=2) + "\n"
    )
    (tokens_dir / "palette.json").write_text(
        json.dumps(strip_extensions(palette), ensure_ascii=False, indent=2) + "\n"
    )
    (tokens_dir / "dimensions.json").write_text(
        json.dumps(strip_extensions(dimensions), ensure_ascii=False, indent=2) + "\n"
    )
    (tokens_dir / "typography.json").write_text(
        json.dumps(strip_extensions(typography), ensure_ascii=False, indent=2) + "\n"
    )
    (tokens_dir / "color.aliases.json").write_text(
        json.dumps(aliases, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    )
    lib_path = args.out_dir / "skala-spectrum.lib.op"
    lib_path.write_text(json.dumps(library, ensure_ascii=False, indent=2) + "\n")
    kinds: dict[str, int] = {}
    for var in library["variables"].values():
        kinds[var["type"]] = kinds.get(var["type"], 0) + 1
    print(f"wrote {lib_path} ({len(library['variables'])} variables: {kinds})")

    merge_script = Path(__file__).with_name("merge_skala_atoms.py")
    if merge_script.is_file():
        spec = importlib.util.spec_from_file_location("merge_skala_atoms", merge_script)
        if spec is None or spec.loader is None:
            raise SystemExit(f"could not load {merge_script}")
        atoms = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(atoms)
        atoms.merge_into(lib_path)


if __name__ == "__main__":
    main()
