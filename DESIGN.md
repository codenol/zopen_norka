---
version: alpha
name: Skala Spectrum
description: Visual identity of Skala^R HTML prototypes (Spectrum, Genome and siblings) as encoded in spectrum-kit. Light theme is default; dark is body.theme-dark.
colors:
  primary: "#2D98B4"
  primary-hover: "#60C4DD"
  primary-active: "#1F849E"
  primary-soft: "#60C4DD"
  primary-disabled: "#C9EDF6"
  secondary: "#00BEC8"
  tertiary: "#157FD4"
  navy: "#11244D"
  canvas: "#EEF1F5"
  surface: "#FFFFFF"
  surface-muted: "#F8F9FB"
  surface-header: "#F2F5F9"
  surface-hover: "#E9ECEF"
  border: "#DFE2E6"
  on-surface: "#3F4146"
  on-surface-secondary: "#75777B"
  on-surface-tertiary: "#8D8F95"
  on-primary: "#FFFFFF"
  error: "#E53334"
  overlay: "#00000026"
  status-success-surface: "#A5F2D4"
  status-success-text: "#093F2B"
  status-warning-surface: "#FFED84"
  status-warning-text: "#3E3605"
  status-critical-surface: "#FCADAD"
  status-critical-text: "#5B0C0D"
  status-info-surface: "#BAE2FF"
  status-info-text: "#06253B"
  status-neutral-surface: "#D5D9DE"
  status-neutral-text: "#3F4146"
  toggle-pressed-bg: "#F0FCFC"
  chip-surface: "#EEF1F5"
  chip-text: "#3F4146"
  tooltip-surface: "#3F4146"
  tooltip-text: "#FFFFFF"
typography:
  headline-lg:
    fontFamily: Roboto
    fontSize: 22px
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: 0.5px
  headline-md:
    fontFamily: Roboto
    fontSize: 18px
    fontWeight: 600
    lineHeight: 1.3
  headline-sm:
    fontFamily: Roboto
    fontSize: 16px
    fontWeight: 600
    lineHeight: 20px
  body-md:
    fontFamily: Roboto
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.4
  body-emphasis:
    fontFamily: Roboto
    fontSize: 14px
    fontWeight: 500
    lineHeight: 1.4
  label-md:
    fontFamily: Roboto
    fontSize: 13px
    fontWeight: 500
    lineHeight: 1.3
  label-sm:
    fontFamily: Roboto
    fontSize: 12px
    fontWeight: 500
    lineHeight: 1.3
  caption:
    fontFamily: Roboto
    fontSize: 12px
    fontWeight: 400
    lineHeight: 18px
  mono:
    fontFamily: Roboto Mono
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.4
rounded:
  none: 0px
  xxs: 2px
  xs: 4px
  sm: 6px
  md: 8px
  l: 10px
  lg: 12px
  card: 16px
  pill: 4000px
spacing:
  spacer-0: 0px
  spacer-50: 1px
  spacer-100: 2px
  spacer-150: 3px
  spacer-200: 4px
  spacer-250: 5px
  spacer-300: 6px
  spacer-350: 7px
  spacer-400: 8px
  spacer-450: 9px
  spacer-500: 10px
  spacer-600: 12px
  spacer-800: 16px
  spacer-1000: 20px
  spacer-1200: 24px
  spacer-1400: 28px
  xs: 4px
  sm: 8px
  md: 12px
  lg: 16px
  xl: 20px
  2xl: 24px
  gutter: 20px
  margin: 20px
  sidebar: 251px
  sidebar-collapsed: 99px
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-primary}"
    typography: "{typography.body-emphasis}"
    rounded: "{rounded.md}"
    height: 38px
    padding: 16px
  button-primary-hover:
    backgroundColor: "{colors.primary-hover}"
    textColor: "{colors.on-primary}"
  button-primary-active:
    backgroundColor: "{colors.primary-active}"
    textColor: "{colors.on-primary}"
  button-primary-disabled:
    backgroundColor: "{colors.primary-disabled}"
    textColor: "{colors.on-primary}"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-emphasis}"
    rounded: "{rounded.md}"
    height: 38px
    padding: 16px
  button-secondary-hover:
    backgroundColor: "{colors.surface-hover}"
    textColor: "{colors.on-surface}"
  button-outline:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.primary}"
    rounded: "{rounded.md}"
    height: 38px
  button-text:
    backgroundColor: transparent
    textColor: "{colors.on-surface-secondary}"
    height: 38px
  button-danger:
    backgroundColor: "{colors.error}"
    textColor: "{colors.on-primary}"
    rounded: "{rounded.md}"
    height: 38px
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    height: 38px
    padding: 12px
  chip:
    backgroundColor: "{colors.chip-surface}"
    textColor: "{colors.chip-text}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.sm}"
    padding: 8px
  badge-success:
    backgroundColor: "{colors.status-success-surface}"
    textColor: "{colors.status-success-text}"
    rounded: "{rounded.pill}"
  badge-warning:
    backgroundColor: "{colors.status-warning-surface}"
    textColor: "{colors.status-warning-text}"
    rounded: "{rounded.pill}"
  badge-critical:
    backgroundColor: "{colors.status-critical-surface}"
    textColor: "{colors.status-critical-text}"
    rounded: "{rounded.pill}"
  badge-info:
    backgroundColor: "{colors.status-info-surface}"
    textColor: "{colors.status-info-text}"
    rounded: "{rounded.pill}"
  badge-neutral:
    backgroundColor: "{colors.status-neutral-surface}"
    textColor: "{colors.status-neutral-text}"
    rounded: "{rounded.pill}"
  link:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.tertiary}"
    typography: "{typography.body-md}"
  field-hint:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface-tertiary}"
    typography: "{typography.caption}"
  divider:
    backgroundColor: "{colors.border}"
    textColor: "{colors.border}"
    height: 1px
  overlay-scrim:
    backgroundColor: "{colors.overlay}"
    textColor: "{colors.on-primary}"
  table-row-alt:
    backgroundColor: "{colors.surface-muted}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-md}"
    height: 44px
  toggle-button:
    backgroundColor: transparent
    textColor: "{colors.primary-soft}"
    typography: "{typography.body-md}"
    rounded: "{rounded.pill}"
    height: 32px
    padding: 12px
  toggle-button-pressed:
    backgroundColor: "{colors.toggle-pressed-bg}"
    textColor: "{colors.primary}"
  tooltip:
    backgroundColor: "{colors.tooltip-surface}"
    textColor: "{colors.tooltip-text}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.md}"
  dialog:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.headline-sm}"
    rounded: "{rounded.card}"
    width: 465px
    padding: 24px
  card:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-md}"
    rounded: "{rounded.card}"
    padding: 24px
  table-header:
    backgroundColor: "{colors.surface-header}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-emphasis}"
    height: 52px
    padding: 16px
  sidebar:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-md}"
    rounded: "{rounded.card}"
    width: 251px
  logo-mark:
    backgroundColor: "{colors.canvas}"
    textColor: "{colors.secondary}"
    typography: "{typography.headline-lg}"
  logo-wordmark:
    backgroundColor: "{colors.canvas}"
    textColor: "{colors.navy}"
    typography: "{typography.headline-lg}"
---

# Skala Spectrum

## Overview

Enterprise operations UI for Skala^R products: cluster consoles, tables of nodes, drawers of object detail. The feel is **dense, calm, and engineered** — a control room, not a marketing site.

Personality: professional, slightly cool, high information density. White cards sit on a cool-gray canvas. Interaction is Bondi Blue, not the Java brand cyan: cyan is identity (logo, switch-on), Bondi is work (buttons, focus, table accent, filter pills).

Target: operators and analysts who live in lists, filters, and overlays for hours. Screens should feel like Spectrum / PrimeReact: 14px Roboto, 38px controls, 8px-radius widgets, 16px-radius shells. Do not drift toward consumer rounded-pill dashboards, neon accents, or generous marketing whitespace.

Default theme is **light**. Dark (`theme-dark`) keeps the same structure; canvas is `$layout/background/default` (`#141824` in the Figma dark set), cards `$card/background/primary`. Do not invent a third palette.

## Colors

Canonical colors live in `design/skala-spectrum.lib.op` (theme axis `Mode`: Light / Dark).

Two layers, as in Figma:

1. **Palette primitives** (`design/tokens/palette.json`) — ramps like `$bondi-blue/600`, `$cool-gray/100`. These do not change with theme.
2. **Semantic colors** (`design/tokens/color.light.json` + `color.dark.json`) — paths like `$button/filled/accent/background/default`. Light and Dark pick *different primitives*. The map is `design/tokens/color.aliases.json`.

Example: primary Light → `$bondi-blue/600` (`#2D98B4`); primary Dark → `$bondi-blue/650` (`#1F849E`). Canvas Light → `$cool-gray/100`; Dark → `$nile-blue/950`.

Where this prose and Figma disagree, **Figma wins**.

The system is a cool-gray field with one working cyan family (Bondi) and a brighter brand cyan (Java).

- **Primary / Bondi 600 (#2D98B4):** The sole driver for primary actions, table accent, pressed filters, sidebar active border (`$button/filled/accent/background/default` → `$bondi-blue/600`). Hover lightens to Bondi 500 (`$bondi-blue/500`, `#60C4DD`). Active darkens to Bondi 650 (`$bondi-blue/650`, `#1F849E`). Disabled primary fills `$bondi-blue/200` (#C9EDF6) — not faded opacity.
- **Primary-soft / Bondi 500 (#60C4DD):** Idle filter pills, dark-theme accent, hover on compact chrome. Lighter than the CTA so unselected filters read as outline, not as buttons.
- **Secondary / Java 550 (#00BEC8):** Brand mark in the logo and the “on” track of switches (`$java/550`). Not for primary buttons.
- **Tertiary / link (#157FD4):** Text links and informational toasts (`$cornflower-blue/600`). Never the page’s main button.
- **Navy (#11244D):** Wordmark next to the Java mark (`$tory-blue/850`). Headlines in content stay ink-gray, not navy.
- **Canvas (#EEF1F5):** Page background (`$layout/background/default` → `$cool-gray/100`). Cards, sidebar, breadcrumbs are white (`$white/950`) on this field.
- **On-surface (#3F4146):** Body text (`$dialog/text/primary` → `$cool-gray/800`). Secondary `#75777B` (`$cool-gray/700`), tertiary/placeholder `#8D8F95` (`$cool-gray/600`).
- **Error (#E53334):** Destructive buttons and critical toast (`$red/600`). Status *pills* use pastel surfaces with dark text, not this saturated red as a fill for table badges.
- **Status pastels:** Success mint, warning yellow, critical pink, info blue, neutral gray — always paired surface + text tokens above. Do not colorize whole rows with these; they are chips and counts.

Borders are `$cool-gray/200` (`#DFE2E6`, `$card/border/primary`), not the primary. Focus rings may use Java or Bondi; do not add extra brand colors. Overlay scrim is `$layout/overlay/background/default` → `$black/200-t` light (`#00000026`) / `$black/500-t` dark (`#000000B2`).

## Typography

Two families from Figma (`$FontFamily/Headlines`, `$FontFamily/body`, `$FontFamily/Mono`): **Roboto** for UI, **Roboto Mono** only for code / tabular figures. Base size of the document is **14px / line-height 1.4**. This is the Spectrum/PrimeReact density — do not bump body to 16px.

- **Headlines:** 22px/700 for the product logo; 18px/600 for card titles; 16px/600/20px for dialog and drawer titles. No display sizes, no serif.
- **Body:** 14px/400. Emphasis and table headers 14px/500–600. Buttons inherit 14px/500.
- **Labels:** 13px/500 for field labels; 12px for hints, chips, captions, tooltips.
- **Mono:** Roboto Mono for codes, IDs, and numeric columns — not for headlines or buttons.
- **Case:** Sentence case in Russian UI. Do not uppercase labels except tiny status codes if the product already does.

Icons are Lucide (or kit SVG) at **16×16** by default; 12px small, 20px in the sidebar utility rail. Missing mapping is a loud magenta (`#E6007A`) so it cannot hide. Checkbox check/dash stay custom `path` nodes (SVG metrics, not lucide `check` / `minus`).

## Layout

Desktop-first operations shell. The page is a padded grid, not edge-to-edge.

- **App shell:** `spx-app` is two columns (sidebar + main) with **20px** padding and **20px** gap, full viewport height, no page scroll on the shell. Main column stacks breadcrumbs, then a white content card that scrolls internally.
- **Sidebar (Skala):** 251px expanded / 99px collapsed; Spectr 2.0 compact is 202 / 50. White, 1px border, ~16–17px radius. Nav items 168×34, 8px radius. Do not use a full-bleed dark sidebar.
- **Spacing scale:** Figma `$spacing/spacer-*` — 0 / 1 / 2 / 3 / 4 / 5 / 6 / 7 / 8 / 9 / 10 / 12 / 16 / 20 / 24 / 28. Prefer 4 / 8 / 12 / 16 / 20 / 24 (`spacer-200` … `spacer-1200`) over arbitrary 10px or 15px. Toolbar gap 12px; card padding 24px; field stack 4px label-to-input, 16px between fields.
- **Tables:** Header 52px on #F2F5F9; body rows min 44px; 16px cell padding. The list stays visible under overlays.
- **CRUD is overlay, not a route:** Create / confirm / short progress → centered dialog (~465px, up to 600px). Long form or row detail → right drawer 400 / 600 / 800 / 960. Toast is outcome, never a confirm. Do not replace the table with a full-page form for 2–10 fields.

Max content is the remaining column after the sidebar, not a 1200px marketing container.

## Elevation & Depth

Hierarchy is **containment + border**, not drop shadows.

- Canvas is cool gray; primary work sits on white cards with 1px `$cool-gray/200` and a whisper shadow `0 1px 4px rgb(0 0 0 / 10%)`.
- Overlays: scrim `$layout/overlay/background/default` (`#00000026` light, `#000000B2` dark). Modal uses `0 8px 32px / 30%`; drawer a left-cast `-8px 0 24px / 12%`.
- Menus: `0 4px 16px / 16%`.
- No glassmorphism, no large colored glows, no stacked fake 3D. Hover is a gray surface tint (`#E9ECEF` or `#EEF1F5`), not a shadow lift.
- Tabs in product UI are **segmented controls**, not underline tabs (especially inside drawers).

## Shapes

Soft-rect, not squircle-consumer.

- **Controls** (buttons, inputs, nav items, menus): **8px** (`$border-radius/M`). Tiny tools and chips: **6px** (`$border-radius/S`). Extra stops: XXS 2, XS 4, L 10, XL 12.
- **Shells** (cards, breadcrumbs, default sidebar, dialogs): **16px** (`$border-radius/XXL`). Drawers round only the inner (left) corners.
- **Pills:** status badges and **filter toggle buttons** only (`$border-radius/XXXXL` = 4000 in Figma — a full-round token, not 999px CSS). Primary actions are never pills.
- **Checkbox** is a **20×20** square, radius **6** (`$border-radius/S`) — not a circle. Avatars are circular. Sidebar **MenuButton** is a **40×40** 8px-radius square (Figma SVG geometry; inspector sometimes says 5px — use **8**). Compact table/tool icon buttons stay 28–32px where those frames say so.

Do not mix 4px “sharp Material” with 24px “iOS blobs” on the same screen.

## Components

Atoms live behind `spx-*` classes. Do not invent parallel primitives (no random `.chip` as a filter, no extra CSS button).

**Buttons (HTML prototypes)** — height 38px. The Figma atom is **32 Large / 24 Small**; prefer the atom when assembling in this kit.

**Button** — control atom, not sidebar chrome. OpenPencil has no Figma variant axis, so Type × Size × Sentiment × State × Icon position become reusable frames. Geometry from Filled SVG: **Large** 351×32 (text 83 / icon+text 105 / icon-only 32) and **Small** 277×24 (61 / 77 / 24). **Outline Large** is the same boxes with 1px centre stroke (SVG path inset 0.5, `#2D98B4`) and no fill — ink is Bondi, not white. **Outline Small** SVG 273×24 is the same 61 / 77 / 24 boxes. Radius `$border-radius/M` both. Large: pad **8 / 16**, icon-only pad **8**, gap **8**, 16×16, 14/500. Small: pad **6 / 8**, icon-only pad **6**, gap **4**, 12×12, 12/500. Figma sample glyph is wine (not in `Icon::from_name`); catalog uses `map-pin`. Override `atom-button-{type}-{size}-{sentiment}-{state}-{layout}-label` (`content`) and `-icon` (`iconFontName`). Colors `$button/{filled,outline,ghost}/{sentiment}/{background,text,border}/{state}`. Filled default has **no** stroke (`white/0-t`). Outline always has 1px centre stroke on the hug frame — do not `align: inside`. **Ghost Large** SVG 351×32 has no shell paths — same hug (83 / 105 / 32), transparent fill, Bondi ink only; hover/active use the tint fills, focus is the Bondi-500 ring. **Ghost Small** SVG 281×24 is the same 61 / 77 / 24 boxes, 12×12 wine, no chrome. Outline default/focus/disabled fills are transparent (`white/0-t`); hover/active pick up the tint tokens. Catalog `Atoms · Button` is Filled then Outline then Ghost, Large then Small, six sentiments × five states × four layouts. Type **Text** waits for its own SVG. Source: `design/atoms/button.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Input** — text field atom, **236×32**, radius `$border-radius/M`, 1px centre stroke. Figma `State` maps to `Input/{Default,Hover,Focused,Filled,Warning,Error,Disabled}`. Pad **8 / 12**, `alignItems: center`, Roboto 14/400. Do not author `x`/`y` on the label (flex centers it). Colors `$input/{background,border,text}/{state}`. Default/filled share the cool-gray-200 stroke (the lavender ring on the Figma screenshot is selection, not the atom). Hover Bondi-600. Focused Bondi-500 + 2px dilate glow `$effects/status/focus/default`. Disabled cool-gray-100 fill. Clip stays off so the focus glow is not cut. Override `atom-input-{state}-label` `content`. No icon on this atom. Sample page `Atoms · Input` is the seven frames, 20px apart. Source: `design/atoms/input.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Input icon** — same 236×32 chrome as Input, plus a 16×16 lucide icon. Figma set is two columns: **Trailing** (icon after the text, `space_between`) and **Leading** (icon before the text, gap 8). Fourteen masters `Input/Icon/{Trailing,Leading}/{Default,Hover,Focused,Filled,Warning,Error,Disabled}`. Do not author `x`/`y` on the label or icon. Icon color `$input/icon/{state}`. Figma sample glyph is wine (not in `Icon::from_name`); catalog uses `map-pin`. Override `atom-input-icon-{placement}-{state}-label` (`content`) and `-icon` (`iconFontName`). Hint `hint message` under each field on `Atoms · Input icon` is catalog chrome (12/400, 8px below the field; warning/error hint matches the stroke) — not a reusable slot. Search/password wait for their own SVG. Source: `design/atoms/input-icon.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Filter pills** — only `.spx-toggle-btn`: 32px tall, pill, Bondi-500 outline at rest, Bondi-600 + ice fill when pressed. Not Chip, not a CTA button.

**Chips & status** — dismissible `Chip/{Default,Disabled}` is hug × 20, rx 10 (see Chip below), not `$border-radius/S`. Color VALUE tags are `Badge/{Color}/…`, not Chip. Count circles are `Badge/Basic`. Table status stays StatusIndicator + pastel counts (22px min).

**Table icons** — **atom**, 12×12 (sort / filter / lock / kebab / grip / chevron) or 16×16 (settings / columns / hide / info). SVG set **296×188**. Figma `State` on Lock/Filter/Sort becomes reusable frames `Table/Icon/{Lock,Filter,Sort}/{Default,Active|Desc|Asc,Disabled}`. Trailing sort+filter in a header is two icons, gap **4**, not a 28px button. Lucide: `funnel` (do not send `filter` — that name is sliders), `arrow-up-down`, `ellipsis-vertical`, `grip-vertical`. Override `-icon`. Sample lives on `Atoms · Table`. Source: `design/atoms/table-icon.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Table header** — **atom**, **52** tall, fill `$table/header/background/default` (`#F2F5F9` in Light), 1px bottom `$table/header/border/default`. Pad **16**. Sample text column **262**. Live 14/500 «Расположение». `Table/Header/TextSortFilter` is `space_between` (label | sort+filter). `Table/Header/CheckboxTextSortFilter` prefixes `Checkbox/Unchecked/Default/Icon` (gap 8). Icon-only headers are **44**. `Table/Header/Checkbox` is **56**, centred instance of the same 20×20 icon (SVG inset 18). Stretch width in the row; 262 is the catalog sample. Source: `design/atoms/table-header.source.svg`.

**Table cell** — **atom**, **44** min, fill `$table/row/background/{default,alternative}` (white / `#F8F9FB` zebra — not hover `#EEF1F5`). 1px bottom `$table/row/border/default`. Pad **16** on text columns (sample **200**). Live 14/400 `$table/text/primary`; links `$table/text/link`. `Table/Cell/Badge` instances `Badge/green/Filled/Text`. Row actions are `Table/Cell/Kebab`, not a pile of Buttons. `Table/Cell/Checkbox` is **56**, centred `Checkbox/Unchecked/Default/Icon` (swap to Checked/Indeterminate). Inline Input, tooltip, and 24px status tags wait. Sample page `Atoms · Table`.

**Dialogs & drawers** — white, 16px radius. Header 56px (74px with subtitle), footer 64px. Title 16/600, subtitle 14/400 `#75777B`. Footer: outline Cancel + primary Submit. Do not put `hidden` on the overlay (it fights `.is-open`).

**Sidebar** — Skala white rail; active item cool-gray fill + Bondi border; logo Java mark + navy wordmark.

**Icons** — default to a Lucide `icon_font` node (`iconFontName` kebab, `iconFontFamily: "lucide"`). Send the lucide name, not outlined SVG: Figma flatten drops the name. Prefer names in `Icon::from_name` so raster preview is not empty dots. Custom `path` only when the glyph is not in lucide (Logo, Checkbox check/dash). MenuButton slot is 20×20 centered in 40×40; MenuItem slot is 16×16.

**MenuButton** — icon-only sidebar chrome, 40×40, radius `$border-radius/M`. OpenPencil has no Figma variant axis, so the four Figma `State` values are reusable frames `MenuButton/Default`, `MenuButton/Hover`, `MenuButton/Active`, `MenuButton/Hover active`. Each master is the square + an `icon_font` named `icon` (sample glyph `bell`). Override instances with `descendants["atom-menu-button-{state}-icon"].iconFontName`. Colors are `$sidebar/menubutton/{background,border,icon}/{default,hover,active,active_hover}`. Active is cool-gray fill + 1px Bondi ring (centre stroke on a 0.5px-inset rect). Hover active is Bondi-500 fill + white icon. Sample page `Atoms · MenuButton` is the four frames only, 20px apart. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**MenuItem** — sidebar row, **168** wide, height **hug**: **34** one line, **52** two lines. Not a Figma variant axis — one master grows when the 120px Body M 14/18 label wraps. The master **is** the horizontal auto-layout: pad **8 / 12**, gap **8**, `alignItems: center` (icon y=18 on wrap), radius `$border-radius/M`. 16×16 Lucide icon. Inner content **144×18** one line / **144×36** two (16+8+120). Clip stays off so hug can grow (Figma clip-content is a no-op on a hug box). Do not nest a second padded row. Figma `State` maps to `MenuItem/Default`, `MenuItem/Hover`, `MenuItem/Active`, `MenuItem/Active hover`. Catalog: Default one-line «Обзор», wrap instance «Диагностика метрики» / `info`, then Hover / Active / Active hover. Override instances with `descendants` on `atom-menu-item-{state}-label` (`content`) and `atom-menu-item-{state}-icon` (`iconFontName`). Hover is cool-gray fill with **no** stroke (the inspector border is a lie — trust the SVG). Active is the same fill + 1px centre Bondi stroke on the hug frame (do not set `stroke.align: inside` — it punches the fill). Active hover is Bondi-500 fill + white icon/text. Default/hover/active ink stays `$sidebar/menuitem/{icon,text}/…` (`#3F4146` in Light), not Bondi. Sample page `Atoms · MenuItem` is the four masters plus the wrap instance, 8px apart. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Logo** — product wordmarks, not a chip. SVG set is **200×432**, seven **200×48** masters with **16px** gap (inspector 240×472 / radius 5 is the Figma set chrome — ignore). No slots: outlined paths are the mark. Figma `Property 1` maps to `Logo/Геном 2.0`, `Logo/Визион`, `Logo/Спектр`, `Logo/Спектр.S3`, `Logo/Лого`, `Logo/Спектр ИИ`, `Logo/Спектр 2.0`. Java caret (`$sidebar/logo/accent/default`, `#00BEC8`) on every variant except **Лого** (`логотип` only). Navy wordmark is `$sidebar/logo/text/default` (`#11244D` in Light). The cyan **2.0** suffix is only on Геном 2.0 and Спектр 2.0 — `.S3` and `.ИИ` stay navy in the SVG. Sample page `Atoms · Logo` is the seven frames only. Source: `design/atoms/logo.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**StatusIndicator** — Spectr status glyphs, two sizes. SVG set is **164×52**: a 16px row and a 20px row, **16px** gap, no set padding (inspector 215×93 / radius 5 / 20px padding is Figma chrome). Figma `Status` × `Size` maps to ten reusable frames `StatusIndicator/{Success,Warning,Process,Error,Zero} {16,20}` in SVG column order (not the inspector dropdown order). Skip unused `Variant6`. Colors are `$indicator/{success,warning,info,critical,unavailable}/background/default` — success/warning/error already bake 80% alpha (`…CC`); Process and Zero are opaque in the SVG. One evenodd path named `icon` per master; do not author it as the full 16×16 / 20×20 or the renderer stretches the ring. Sample page `Atoms · StatusIndicator` is the ten frames only. Source: `design/atoms/status-indicator.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Status** — sidebar presence dots, not the ring glyphs. SVG set is **40×132**: three **40×40** tiles with **6px** gap (inspector 80×172 / radius 5 is Figma chrome). Figma `Property 1` maps to `Status/Default`, `Status/Lock`, `Status/Danger` in SVG order (green, lock, red — not the inspector dropdown order). Each master is a transparent 40×40 frame with a 20×20 circular `dot` (rounded frame). Default/Danger are opaque `$sidebar/status/indicator/{default,danger}` (`#56B361` / `#D41F20` in Light — not the 80% StatusIndicator tokens). Lock is `$sidebar/status/indicator/lock` (`#D5D9DE`) plus Figma's `lock-keyhole` instance nested in the dot: a 12×12 frame at (14, 14) wrapping an evenodd padlock path (tight 10×11 bbox, `$cool-gray/700`). Sample page `Atoms · Status` is the three frames only. Source: `design/atoms/status.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Avatar** — sidebar initials, **atom** (tile + disc + live text), not a molecule. Master is **40×40** with a **32×32** disc at (4, 4) and 4px safe field — same 40px minibar grid as Status and MenuButton (the inspector often selects only the disc). Disc `cornerRadius` 16, no stroke. Bind `$sidebar/avatar/background/default` (`#439EE4` / cornflower-550); Dark on `button/primary/info/default-bg` differs. Initials `$sidebar/avatar/text/default` (`#FFFFFF`), 14/500 Roboto, sample «AB». Rounded inner frame + auto-layout center — do not use a sibling ellipse. Sample page `Atoms · Avatar` is the one frame. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Divider** — sidebar hairline, **atom**. Figma is **200 Fill × 1 Hug** with a Fill of `$sidebar/border/default` (`#DFE2E6` in Light) — not a stroke, and not `$sidebar/divider-horizontal/default` (that alias is `#3F4146` in Light). Catalog master `Divider/Default` is a 200×1 filled frame. The footer «Свернуть» row is a `MenuItem` with a chevron, not a new atom. Sample page `Atoms · Divider` is the one frame. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Sidebar 2.0** — **organism** (page region), not an atom. Light Genome 2.0 sample is reusable `Sidebar/Default` on `Organisms · Sidebar`: 251×814, radius `$border-radius/XXL`, fill `$sidebar/background/default`, 1px centre-stroke `$sidebar/border/default`. Minibar 48×812 at (1, 1), padding 8/4. Menu is **200×812** at (50, 1), vertical **gap 8**, pad 0 — not `space_between`. Children: `Header` (logo 200×48 + divider, hug, gap 0), `Menu slot` Fill (**704** tall, pad **[0, 16]**, gap 0, items **168**), footer `Divider`, `bottom-block` (pad [0, 16], collapse `MenuItem` `chevron-left`). Inspector 1px outside stroke on Menu is the minibar join — a 1px rule at x=49, not a second shell stroke (and not a second 75% fill on Menu). Source: `scripts/skala_organism_sidebar.py`. Rebuild: `python3 scripts/merge_skala_atoms.py`. Dark and status-axis organism variants are later.

**Breadcrumbs** — **molecule** (trail of labels + 4px dots in a shell), not an atom. SVG set is **347×304**: five **48** tall tiles (SVG rect 47 at y=0.5) with **16px** gap (inspector 546×344 / radius 5 is Figma chrome). Figma `lots` maps to reusable frames `Breadcrumbs/1`, `Breadcrumbs/2`, `Breadcrumbs/3`, `Breadcrumbs/4`, `Breadcrumbs/4+`. Hug width (82 / 145 / 212 / 279 / 346 with sample «Label»), radius `$border-radius/XXL`, 24px horizontal padding, 16px gap. Fill `$breadcrumbs/background/default` (`#FFFFFFBF` in Light), 1px centre-stroke `$breadcrumbs/border/default`. Last crumb `$breadcrumbs/text/default` (`#3F4146`); ancestors and dots `$breadcrumbs/text/secondary` (`#75777B`). Live 14/500 Roboto; separator is a 4×4 rounded frame, not a middle-dot glyph and not a sibling ellipse. Skip Figma blur. Hover/icon tokens exist but this set has no hover or icon. Override instances with `descendants` on `molecule-breadcrumbs-{lots}-label-{i}` (`content`). Sample page `Molecules · Breadcrumbs` is the five frames only. Source: `scripts/skala_molecule_breadcrumbs.py`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Pagination items** — **atoms**, not Button/Icon. SVG set: numbers **32×136** (Default / Hover / Selected, catalog gap 20), arrows **77×131** (Prev + Next columns, Default / Hover / Disabled). Hit box **32×32**, radius `$border-radius/M`. Number Default has no chrome (transparent `$pagination/item/number/background/default`); Hover and Selected fill `$pagination/item/number/background/{hover,selected}` (`#EEF1F5`). Live 14/400 Roboto, override `atom-pagination-item-number-{state}-label`. Arrows are Lucide `chevron-left` / `chevron-right` (swappable); Disabled ink `$pagination/item/arrow/icon/disabled` (`#CDD0D7`). Ellipsis is Lucide `ellipsis`. **PageSize** is the closed dropdown chrome (hug × 32, pad 8/12, `$dropdown/*`). Open list is `ContextMenu/PageSize`, **4px** below the trigger (`Pagination/PageSize/Open`). Sample page `Atoms · Pagination`. Source: `scripts/skala_atom_pagination.py`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Pagination** — **molecule** (the assembled bar). SVG **364×32**: disabled prev + selected `1` + `2 3 4` + ellipsis + `45` + next + PageSize `10`. Horizontal auto-layout, **gap 8**, hug width. Place under a table; override page digits via nested `descendants` on `molecule-pagination-default-page-*`. Do not rebuild from Button/Icon. Compact «Page N of M» waits for its SVG. Sample page `Molecules · Pagination` also has `Pagination/PageSize/Open`. Source: `scripts/skala_molecule_pagination.py`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Context menu items** — **atoms**. Base SVG **352×348** (filter pad; menu **320×316**, rx 8). Rows **30** tall, radius `$border-radius/M`, horizontal pad 8. Default/Hover/Selected/Disabled for Text, WithIcon, and WithChip; Selected is `$context-menu/menu-item/background/selected` (`#EEF1F5`) plus 1px centre `$context-menu/menu-item/border/selected` (`#2D98B4`). Hover is fill only. Live 14/400 Roboto, sample «ContextMenuItem». WithIcon is 16×16 lucide `map-pin` + gap 8 (inferred from Button Large until a WithIcon SVG lands). WithChip trails `Chip/Default` (`Chip/Disabled` on the disabled row). Semantic Danger/Info/Success recolor **only the icon**. Width `fill_container` (catalog column 304). Sample page `Atoms · ContextMenu`. Source: `scripts/skala_atom_context_menu.py`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Context menu** — **molecule**. White `$context-menu/background/default` shell, pad 8, rx 8, shadow offsetY 6 / blur 8 / `#00000014`. `clipContent` true. Variants: `Base`, `WithIcon`, `Badges` (trailing Chip), `Search`, `SearchIcon`, `SearchBadges`, `Semantic`, `PageSize` (160 wide, 10/20/50/100). Place **4px** below the trigger. Sample page `Molecules · ContextMenu`. Source: `scripts/skala_molecule_context_menu.py`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Chip** — **atom**, dismissible pill. SVG set **60×60**: Default at y=0, Disabled at y=40 (catalog gap 20). Hug × **20**, rx **10** (half height — not `$border-radius/S` 6). Pad 4/8, gap 8, 12×12 lucide `x-circle` then live 12/400 «Text». Fill `$chip/background/{default,disabled}` (`#EEF1F5` / `#F8F9FB`), text `$chip/text/*` (`#303134` / `#A8AAB0`), icon `$chip/icon/*` (`#75777B` / `#DFE2E6`). Override `atom-chip-{state}-label` / `-icon`. Not a filter toggle (those are `.spx-toggle-btn`). Not the color VALUE tag matrix (that is Badge). Sample page `Atoms · Chip`. Source: `scripts/skala_atom_chip.py`, `design/atoms/chip.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Checkbox** — **atom**, 20×20 control, radius **6**. Matrix SVG **772×280**: Unchecked / Checked / Indeterminate × Icon + Text, rows Default / Hover / Focus / Disabled / Warning (catalog gap 20). Fill/stroke `$checkbox/{selection}/{background,border}/{state}`; label `$checkbox/{selection}/text/{state}` live 14/400 «Text», gap **8**. Checked/indeterminate glyphs are evenodd **paths** (white check / 10×1.33 dash), not lucide — do not author the path node as the full 20×20. Focus glow is 2px spread `$effects/status/focus/default` (`clipContent` false). Warning has no unique hover; disabled uses the base mute, not orange. TextInfo is the **84×24** SVG: same row plus 16×16 lucide `info` (`$table/icon/info`). 45 masters `Checkbox/{Unchecked,Checked,Indeterminate}/{State}/{Icon,Text,TextInfo}`. Override `-label` / `-icon`. Sample page `Atoms · Checkbox`. Source: `scripts/skala_atom_checkbox.py`, `design/atoms/checkbox-info.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Badge-basic** — **atom**, count circle. SVG set **45×16**: Base at x=0, MassAction at x=29 (catalog gap **13**). **16×16**, rx **8**, 1px centre white stroke. Live 10/500 «1» (SVG outlines the digit — do not keep paths). Fill `$badge-basic/{base,massAction}/background/default` (`#3F4146` / `#E53334` in Light), text/stroke `$badge-basic/…/{text,stroke}/default` (white). Override `atom-badge-basic-{base,mass-action}-label`. Overlay on a button at top-right; do not nest inside a clipping Button. Sample page `Atoms · BadgeBasic`. Source: `scripts/skala_atom_badge_basic.py`, `design/atoms/badge-basic.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Badge** — **atom**, color VALUE tag. Figma property panel says Chip; tokens are `$badge/{color}/…`. Not dismissible Chip and not Badge/Basic. SVG matrix **334×451**: 15 colors × filled/outline × text / text+icon / icon = **90**. Row step **31** (h **17**, catalog gap 14), rx **8.5**. Filled Text+Icon puts the icon after the label; Outline TextIcon puts it before. Live 11/500 «VALUE». Figma wine glyph is not in `Icon::from_name`; catalog uses `map-pin`. Colors in SVG order: gray-strong, gray, nile-blue, tory-blue, cornflower-blue, bondi-blue, java, green, shamrock, yellow, orange, red, rose, violet, gray-warm. Override `-label` / `-icon`. Sample page `Atoms · Badge`. Source: `scripts/skala_atom_badge.py`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Button with badge** — **molecule**. SVG **85×92**: Large Filled Accent Text (83×32 at y=8) plus `Badge/Basic/Base` 16×16 at (69, 0). Wrapper `layout: none`, `clipContent: false`. Catalog also shows the plain Button at y=60 (gap 20) — not a second master. Sample is Large Accent Default Text only; do not duplicate the 720 Button variants. 99+ pill, tooltip «1225», and MassAction-on-button wait for their SVGs. Sample page `Molecules · ButtonBadge`. Source: `scripts/skala_molecule_button_badge.py`, `design/molecules/button-badge.source.svg`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Layout** — **template** (Frost: page chassis that places organisms), not an organism and not a page. Figma «Структура и отступы» is reusable `Layout/Default` on `Templates · Layout`: **1440×850**, fill `$layout/background/default` (`#EEF1F5` in Light). Gutters **20** top/left and **16** right/bottom/gap. `Sidebar/Default` at (20, 20) 251×814; content column at (287, 20) 1137×814. Column: `Breadcrumbs/3` (hug) then `Main container` **750** tall (`$container/background/default`, 1px `$container/border/default`, radius 16). Numeric sizes, not `fill_container` through the root padding — otherwise the slot swallows the 16px right/bottom gutter. The Figma lavender block is a slot annotation; the kit uses container tokens. Product-specific sidebar copy from that file (Спектр / «Сервисы СУБД») is a later page instance; this template reuses Genome `Sidebar/Default`. **Agent contract:** do not rebuild this chassis. Adapt logo / nav / breadcrumbs for the screen, then fill only the content area (`Main container` / `tpl-layout-main`). Source: `scripts/skala_template_layout.py`. Rebuild: `python3 scripts/merge_skala_atoms.py`.

**Tooltip** — cool-gray-800 (`#3F4146`) on white 12px text.

**Toast** — strip colors: success `#2D8B38`, warning `#F59E0B`, critical `#E53334`, info `#157FD4`. Host in the corner; never blocks the table.

## Do's and Don'ts

- Do keep body at 14px Roboto and controls at 38px — this *is* the product density.
- Don't use Java (#00BEC8) as the primary button fill; that fill is Bondi (#2D98B4).
- Do put create / edit / confirm / row detail in a dialog or drawer over the list.
- Don't turn CRUD into a full-page form, a centered “form card” without a scrim, or a toast that asks “Delete?”.
- Do use `.spx-toggle-btn` for filter pills; don't reuse `.spx-chip` or `.spx-btn` as filters.
- Do use pastel status tokens for badges; don't paint entire table rows in those fills.
- Don't introduce a third typeface, 16px body, pill-shaped primary buttons, or decorative gradients. Roboto + Roboto Mono is the full set.
- Don't copy the Forage/OpenPencil cream-and-forest demo — that is not Skala.
- Do leave the canvas cool gray so white cards read as surfaces; don't make the whole viewport white.
- Do default to light theme tokens unless the screen is explicitly `theme-dark`.
- Do keep Bondi and Java as in the kit even where WCAG AA is short (Bondi on white is ~3.35:1). Don't "fix" contrast by swapping in unrelated blues.
