# Nota Visual Design System

- Status: Accepted
- Last updated: 2026-08-27
- Owners: Nota maintainers

This specification is the source of truth for Nota's in-product visual
language. It covers the Windows desktop client, including auxiliary windows.
It is intentionally separate from marketing artwork and release assets.

## Product Character

Nota should feel calm, trustworthy, local, and precise. The visual language
uses warm neutral surfaces and a restrained jade accent so recording state and
meeting content remain more prominent than decoration. Interfaces should feel
native to Windows 11 without impersonating a Microsoft product.

The foundation follows the Windows typography and four-pixel spacing ideas in
[Fluent 2](https://fluent2.microsoft.design/), while Nota owns its palette,
component styling, and product identity. Accessible interaction primitives may
come from [Radix](https://www.radix-ui.com/primitives); adopting a primitive
does not authorize importing that library's default visual identity.

## Sources of Truth

1. `src/design-tokens.css` contains executable global and semantic tokens.
2. This document defines when each token and component treatment is used.
3. Shared React components and CSS component classes encode recurring UI
   patterns.
4. Individual pages must not invent a parallel color or type system.

The token dependency direction is:

```mermaid
flowchart LR
    F["Foundations<br/>type, color, spacing, shape"] --> S["Semantic tokens<br/>text, surface, border, action"]
    S --> C["Shared components<br/>button, menu, tooltip, dialog"]
    C --> P["Product pages"]
```

## Typography

Use `Segoe UI Variable` on supported Windows versions, then `Segoe UI` and
`Microsoft YaHei UI` as fallbacks. Chinese and Latin content must use the same
semantic type roles. Read-only code and structured request previews use the
shared `--font-family-mono` stack; ordinary controls and content must not use
the monospace role.

Read-only JSON trees use the shared `JsonTreeView` component and the
`--color-syntax-*` semantic roles for keys, strings, numbers, booleans, nulls,
and punctuation. Long values must wrap, nested values must remain keyboard
expandable, and product pages must not import a third-party JSON theme.

| Role | Size / line height | Weight | Usage |
|---|---|---|---|
| Caption | 12 / 16 px | 400 or 600 | Tooltips, timestamps, badges, secondary metadata, dense toolbar actions |
| Body | 14 / 20 px | 400 | Menus, controls, forms, ordinary copy |
| Body strong | 14 / 20 px | 600 | Button labels, list titles, emphasized body text |
| Body large | 18 / 24 px | 400 or 600 | Prominent values and compact window headings |
| Subtitle | 20 / 28 px | 600 | Section and panel headings |
| Title | 28 / 36 px | 600 | Page headings |
| Large title | 40 / 52 px | 600 | Exceptional hero content only |

Rules:

- Product UI text must not be smaller than Caption. Do not introduce 8–11 px
  text to fit a layout; revise the layout instead.
- Tooltips use Caption. Context menus and form controls use Body.
- Use regular, medium, semibold, or bold token weights. Avoid intermediate
  values such as 550 or 650.
- Sentence case is preferred. Weight and semantic color establish hierarchy;
  all-caps text and letter spacing must not be used as substitutes.

## Color

Components consume semantic tokens, not literal colors. The principal roles
are:

| Role family | Intended use |
|---|---|
| `--color-text-*` | Text and foreground hierarchy |
| `--color-surface-*` | Canvas, panels, raised surfaces, hover and status fills |
| `--color-border-*` | Structural, subtle, strong, and status borders |
| `--color-control-*` | Primary, secondary, and destructive actions |
| `--color-icon-*` | Neutral and brand icon hierarchy |

The jade brand color is reserved for primary actions, selected state,
keyboard focus, and positive active state. Warning, danger, success, and info
must use their semantic roles and must not be communicated by color alone.

Normal text must reach a contrast ratio of at least 4.5:1. Large text must
reach at least 3:1. Focus indicators and meaningful non-text controls must be
visually distinguishable in every state.

## Spacing and Layout

Use the four-pixel token rhythm in `src/design-tokens.css`. Half steps of 2,
6, and 10 px are allowed only for compact alignment and icon optical balance.

- 4–8 px: tightly related content inside a control.
- 12–16 px: normal component padding and related groups.
- 20–24 px: sections and panel boundaries.
- 28–32 px: page-level separation.
- 40 px and above: exceptional layout separation.

Fixed dimensions may be used for media, window constraints, grid tracks, or
control geometry, but surrounding spacing must use tokens.

## Shape, Elevation, and Motion

- Small radius (6 px): menu items, tags, compact controls.
- Medium radius (8 px): standard buttons and inputs.
- Large radius (12 px): cards, dialogs, and panels.
- Extra-large radius (16 px): major containers only.
- Fully rounded geometry is limited to avatars, status dots, pills, and round
  playback controls.
- Use the four elevation tokens. Do not encode arbitrary shadow colors in
  component CSS.
- Motion should explain state change. Use the fast or normal duration token,
  and honor `prefers-reduced-motion`.

## Controls and Iconography

Lucide remains Nota's product icon set. Use 14 px icons in compact controls,
16 px by default, and 20 px for navigation or prominent status. Icon-only
buttons need an accessible name and an `AppTooltip`; visible text buttons do
not need a duplicate tooltip.

Application identity is not a Lucide action icon. The sidebar imports the
packaged `src-tauri/icons/128x128.png` directly, with no replacement glyph,
extra background or clipped silhouette. Installer, window and tray icons use
the same packaged microphone artwork; the tray embeds `icons/32x32.png`.
Idle/completed/recovering tray states show the unmodified app mark. Recording,
preparing and finalizing add a lower-right recording-red badge; paused adds
a paused-amber badge. Interrupted/error states and pending capture decisions
show a warning-amber badge, with pending decisions taking priority. Badges have
an anti-aliased light separator and use native equivalents of the existing
status color tokens. Existing tooltip/menu text supplies the precise state.

| Control | Standard |
|---|---|
| Compact icon button | 28 px minimum visual target, 14 px icon |
| Compact text button | 34 px high, Caption strong label, 14 px icon |
| Default icon button | 32 px minimum visual target, 16 px icon |
| Default button/input | 32–36 px high, Body or Body strong label |
| Tooltip | Caption, shown on hover and keyboard focus |
| Context menu item | Body, 32 px minimum height |

Disabled controls must remain identifiable, expose an explanation when the
reason is not obvious, and must not rely on reduced opacity as their only
state distinction.

Compact text buttons are reserved for dense toolbars and inline status banners.
Use the shared `button compact` variant instead of recreating its dimensions in
page-specific CSS.

## Component and Page Rules

- Reuse an existing shared component or class before adding page-specific
  variants.
- New component variants must be named by purpose (`danger`, `secondary`,
  `compact`), not by a literal appearance (`red`, `12px`, `dark-gray`).
- Page CSS may control composition and layout, but shared component anatomy,
  type, color, state, radius, and elevation belong to shared styles.
- Portalled UI such as tooltips, menus, dialogs, and popovers must use tokens
  and must appear above scroll containers without being clipped.
- Keyboard focus must be visible. Hover-only behavior is not sufficient.

### Capture Target Select

The application capture picker uses a Radix select primitive so its custom
visual treatment retains listbox keyboard navigation, focus management, and
typeahead behavior. The closed trigger remains one line and shows the selected
application icon beside the existing executable/title label. The portalled
list shows the window title as Body text and the executable filename as Caption
metadata; both must truncate without displacing the icon or selected marker.

Application icons use the existing 20 px icon role with a subtle structural
border. They are decorative to assistive technology. When Windows cannot
provide an icon, use the neutral Lucide `AppWindow` fallback rather than an
empty or broken image. A target that stops running retains its last in-memory
icon and appends “（未运行）”; this visual state must not select another target.
The picker must continue to expose a visible focus ring, an accessible label,
and distinct highlighted, selected, empty, and refreshing states.

### Settings Center

Settings is a full-height workspace inside the application shell. The global
application sidebar remains visible, while the normal page header and footer
are removed so the settings hierarchy owns the available height.

- The settings category navigation is a persistent `200 px` grid track. It
  never collapses or hides, including on Provider and template management
  routes. A selected management route highlights its parent category.
- Ordinary preference pages use one readable column of stacked
  `SettingsCard` rows. A card contains one icon, a primary label, optional
  supporting text, and one trailing control or navigation affordance. It must
  not become a second nested navigation system.
- Provider and template management pages use three conceptual columns: the
  persistent category navigation, a `260 px` resource list, and a scrollable
  detail editor. ASR and LLM editors share this shell and its feedback/action
  treatments, but keep type-specific fields and validation.
- Explicit-save editors keep their actions in a stable detail footer. Built-in
  templates use the same detail region in a read-only state; custom templates
  may be edited, saved, or archived.
- At the `980 px` minimum application width, the category and resource tracks
  reduce to approximately `168 px` and `220 px`. Multi-column forms become one
  column. The category navigation remains visible and no settings route may
  introduce horizontal page overflow.
- The category list, resource list, and detail region own independent vertical
  scrolling where needed. Chinese long text may wrap in descriptions, while
  paths and compact resource metadata use tokenized truncation treatments.

Settings uses existing semantic surface, border, status, spacing, and focus
roles. The workspace geometry does not introduce a new visual token role.

### Recording Detail Workspace

The recording library uses the full application height with the main sidebar
retained and the redundant global header/footer removed. The record list is a
280 px track (240 px at widths up to 1100 px). The icon button before the detail
title toggles only that track, with matching tooltip and accessible labels
“折叠录音列表” / “展开录音列表”. It exposes the list's expanded state and remains
available when the list is hidden.
The compact title, detail tabs, contextual toolbar, reading region, and native
audio player follow DOM order. The list and reading region scroll independently;
the outer detail pane must not scroll. Playback remains mounted across tabs.

AI documents use shared select styling for document/version selection instead
of a permanent nested document sidebar. AI revision and copying stay directly
available; low-frequency file and generation actions use a keyboard-accessible
disclosure. Generation details open in a right-hand native dialog drawer with
focus containment and restoration. Generation configuration dialogs retain
their original fields and controls. Their rounded outer shell clips overflow;
only the active form/request panel scrolls, leaving the title, tabs and footer
visible. Scrollbars stay inside the shell padding, clear of its rounded corners.
Close controls use Lucide `X`; JSON tree disclosure slots use the bundled Lucide
chevron SVG as a semantic-color CSS mask without replacing tree keyboard behavior.
Both generation overlays dismiss on primary clicks that start and end on their
backdrop. Inside clicks and text-selection drags must not dismiss them; generation
submission disables close, cancel and backdrop dismissal together.

At 1280×800 and 980×640, normal completed-document content targets at least
70% and 60% of workspace height respectively. Necessary warnings may reduce
that area. Controls must not wrap into multiple toolbar rows, and long titles,
models, and paths must not cause page-level horizontal overflow. Use existing
semantic tokens; do not shrink body type to achieve density. The playback
footer retains the local-recording/network-use reminder.

## Change and Review Checklist

For every visual change:

1. Identify the semantic role before selecting a token.
2. Reuse or extend a shared component pattern.
3. Verify normal, hover, focus, disabled, loading, empty, error, and long-text
   states that apply.
4. Verify Chinese copy, 125% Windows display scaling, and keyboard navigation.
5. Run `npm run check:design` and the checks appropriate to the changed area.
6. Update this document and the token file together when the visual language
   gains a new durable role.
