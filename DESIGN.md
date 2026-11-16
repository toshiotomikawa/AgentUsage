---
version: "0.1.0"
name: AgentUsage
description: Minimalist desktop quota dashboard in Rust (eframe/egui) with light and dark themes, concentric status rings, and state-only color coding.
colors:
  primary: "#009051"
  warn: "#c69035"
  crit: "#770001"
  empty: "#8f9394"
  close-hover: "#c42b1c"
  bg-light: "#f1f3f6"
  bg-low-light: "#e6e9ee"
  card-light: "#ffffff"
  border-light: "#e1e4e9"
  border-hover-light: "#cbd0d7"
  text-light: "#16181c"
  muted-light: "#7d838c"
  faint-light: "#afb5bd"
  track-light: "#eceef2"
  pace-light: "#17171a"
  bg-dark: "#111215"
  bg-low-dark: "#0b0c0e"
  card-dark: "#17171a"
  card-hover-dark: "#1b1b1f"
  border-dark: "#242429"
  border-hover-dark: "#34343a"
  text-dark: "#ececee"
  muted-dark: "#8c8c93"
  faint-dark: "#55555c"
  track-dark: "#26262b"
typography:
  card-title:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 600
    fontSize: 15.5px
  no-data:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 400
    fontSize: 14px
  pct:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 600
    fontSize: 13.5px
  body:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 400
    fontSize: 13px
  titlebar:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 600
    fontSize: 12.5px
  note:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 400
    fontSize: 12.5px
  countdown:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 400
    fontSize: 12px
  menu-item:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 600
    fontSize: 12px
  menu-note:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 400
    fontSize: 11.5px
  group-label:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 600
    fontSize: 11px
  section-label:
    fontFamily: "Google Sans, Segoe UI"
    fontWeight: 600
    fontSize: 10.5px
rounded:
  none: 0px
  bar: 3px
  placeholder: 5px
  menu-item: 7px
  card: 10px
  circle: 14px
spacing:
  btn-gap: 12px
  grid-gap: 16px
  card-pad: 24px
  group-gap: 26px
  row-step: 50px
  outer-max: 56px
components:
  window-light:
    backgroundColor: "{colors.bg-light}"
    textColor: "{colors.text-light}"
    rounded: "{rounded.none}"
  window-low-light:
    backgroundColor: "{colors.bg-low-light}"
  window-dark:
    backgroundColor: "{colors.bg-dark}"
    textColor: "{colors.text-dark}"
    rounded: "{rounded.none}"
  window-low-dark:
    backgroundColor: "{colors.bg-low-dark}"
  titlebar:
    typography: "{typography.titlebar}"
    height: 38px
  close-button-hover:
    backgroundColor: "{colors.close-hover}"
    textColor: "{colors.card-light}"
    rounded: "{rounded.none}"
    height: 38px
    width: 46px
  card-light:
    backgroundColor: "{colors.card-light}"
    textColor: "{colors.text-light}"
    typography: "{typography.card-title}"
    rounded: "{rounded.card}"
    padding: 24px
  card-dark:
    backgroundColor: "{colors.card-dark}"
    textColor: "{colors.text-dark}"
    typography: "{typography.card-title}"
    rounded: "{rounded.card}"
    padding: 24px
  card-hover-dark:
    backgroundColor: "{colors.card-hover-dark}"
    textColor: "{colors.text-dark}"
    rounded: "{rounded.card}"
  border-light:
    backgroundColor: "{colors.border-light}"
  border-hover-light:
    backgroundColor: "{colors.border-hover-light}"
  border-dark:
    backgroundColor: "{colors.border-dark}"
  border-hover-dark:
    backgroundColor: "{colors.border-hover-dark}"
  round-button-light:
    backgroundColor: "{colors.card-light}"
    textColor: "{colors.text-light}"
    rounded: "{rounded.circle}"
    height: 28px
    width: 28px
  round-button-dark:
    backgroundColor: "{colors.card-dark}"
    textColor: "{colors.text-dark}"
    rounded: "{rounded.circle}"
    height: 28px
    width: 28px
  provider-disc-light:
    backgroundColor: "{colors.track-light}"
    textColor: "{colors.text-light}"
    rounded: "{rounded.circle}"
    height: 28px
    width: 28px
  provider-disc-dark:
    backgroundColor: "{colors.track-dark}"
    textColor: "{colors.text-dark}"
    rounded: "{rounded.circle}"
    height: 28px
    width: 28px
  menu-light:
    backgroundColor: "{colors.card-light}"
    textColor: "{colors.text-light}"
    typography: "{typography.menu-item}"
    rounded: "{rounded.card}"
    padding: 6px
    width: 200px
  menu-dark:
    backgroundColor: "{colors.card-dark}"
    textColor: "{colors.text-dark}"
    typography: "{typography.menu-item}"
    rounded: "{rounded.card}"
    padding: 6px
    width: 200px
  caption-light:
    textColor: "{colors.muted-light}"
    typography: "{typography.countdown}"
  caption-faint-light:
    textColor: "{colors.faint-light}"
    typography: "{typography.menu-note}"
  caption-dark:
    textColor: "{colors.muted-dark}"
    typography: "{typography.countdown}"
  caption-faint-dark:
    textColor: "{colors.faint-dark}"
    typography: "{typography.menu-note}"
  pace-tick-light:
    backgroundColor: "{colors.pace-light}"
  quota-bar-light:
    backgroundColor: "{colors.track-light}"
    rounded: "{rounded.bar}"
    height: 6px
    typography: "{typography.body}"
  quota-bar-dark:
    backgroundColor: "{colors.track-dark}"
    rounded: "{rounded.bar}"
    height: 6px
    typography: "{typography.body}"
  status-ok:
    backgroundColor: "{colors.primary}"
  status-warning:
    backgroundColor: "{colors.warn}"
  status-critical:
    backgroundColor: "{colors.crit}"
  status-empty:
    backgroundColor: "{colors.empty}"
---

## Overview

AgentUsage is a custom-painted desktop dashboard built in Rust with `eframe` and `egui` (version 0.37) to display remaining provider quotas on a single screen without scrolling. It uses a frameless window with custom chrome, light and dark palettes, and neutral card surfaces where color is reserved strictly for quota state.

## Colors

Neutral surfaces separate the window background, cards, and tracks, while semantic colors encode quota state across every provider:

- **On pace (`#009051`):** healthy remaining quota matching or ahead of elapsed window time, and active card switch tracks.
- **Attention (`#c69035`):** 25% or less remaining, or usage more than 15% ahead of elapsed pace.
- **Critical (`#770001`):** under 10% remaining.
- **Depleted (`#8f9394`):** under 0.5% remaining until reset.
- **Close hover (`#c42b1c`):** title bar close button fill on pointer hover.

The light palette sets a vertical top-to-bottom pearl gray gradient (`#f1f3f6` to `#e6e9ee`) beneath pure white cards (`#ffffff`) with `#e1e4e9` borders. The dark palette uses a vertical charcoal gradient (`#111215` to `#0b0c0e`) beneath `#17171a` cards (`#1b1b1f` on hover) with `#242429` borders.

## Typography

Runtime font loading reads `GoogleSans-Regular.ttf` and `GoogleSans-SemiBold.ttf` from system or user font directories, falling back to `Segoe UI` (`segoeui.ttf`, `seguisb.ttf`) and `egui` built-in proportional fonts. Monospace fonts are not used.

- **Card titles:** 15.5px Semibold.
- **Percentages:** 13.5px Semibold, right-aligned beside muted 12px countdowns.
- **Window labels and account qualifiers:** 13px Regular.
- **Group headers and menu section captions:** uppercase Semibold at 11px and 10.5px.

## Layout

A custom 38px title bar sits above the main dashboard, with 46px-wide window controls on the right and an app logo plus 12.5px title on the left. Two 28px round buttons (refresh and menu) align above the top-right corner of the card grid with a 12px gap.

Cards arrange in an alphabetical row-major grid up to 1280px wide, using 1 column below 620px, 2 columns at 620px+, and 3 columns at 980px+, with a 220px minimum card width. Every card in a row shares the tallest card's height, and an incomplete final row stays left-aligned. When cards overflow vertically, density scales from `1.0` down to `0.6` in `0.05` steps, and window zoom steps down from `1.0` to `0.5` so everything fits without scrolling.

## Elevation & Depth

Surfaces rely on 1px borders (`#e1e4e9` light, `#242429` dark) rather than heavy shadows. Light-mode cards add a subtle 2px downward hover shadow (`black` at alpha 10), and the foreground dropdown menu casts a soft shadow (`offset: [0, 6]`, `blur: 18`, alpha 26 light / 90 dark).

## Shapes

- **Window frame and window buttons:** 0px corners, 1px hairline border (`border-hover`), 5px invisible resize edges.
- **Cards and dropdown menu:** 10px corner radius (`CARD_R`), 24px card padding, 6px menu padding.
- **Round buttons, provider icon discs, and status rings:** 28px diameter (`CIRCLE`), 2px ring stroke width (`RING_W`) across up to three concentric rings (hours, week, month).
- **Quota bars:** 6px height (`BAR_H`) with 3px pill corners and a vertical pace tick centered on the bar.

## Do's and Don'ts

- Do keep provider identity in icons and card titles rather than per-provider color tints; reserve color exclusively for quota status.
- Do normalize every quota bar and concentric ring to percentage remaining with a pace tick when a window is active.
- Do hide secondary details (exact reset timestamp, run-out forecast, status label) inside hover tooltips.
- Don't introduce scrolling or fixed monospace tables; scale grid density and zoom to fit the viewport.
- Don't use decorative borders or heavy drop shadows in dark mode.
