---
name: Mnemora
description: A chromatic memory-index workstation with seven multi-color theme systems.
colors:
  app-ground: "#ebeef8"
  content-surface: "#fbfbff"
  raised-surface: "#ffffff"
  sidebar-surface: "#e7e4f3"
  activity-rail: "#2e2850"
  text-primary: "#23213a"
  text-secondary: "#514e68"
  text-muted: "#6d6985"
  border-default: "#cbc7dc"
  overview: "#ad6100"
  chat: "#00786f"
  notes: "#c43b66"
  literature: "#315fbd"
  english: "#477922"
  settings: "#6e4aaa"
  on-accent: "#ffffff"
  danger: "#b13c3c"
typography:
  headline:
    fontFamily: '"Segoe UI Variable", "Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif'
    fontSize: "26px"
    fontWeight: 680
    lineHeight: 1.2
    letterSpacing: "0"
  title:
    fontFamily: '"Segoe UI Variable", "Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif'
    fontSize: "16px"
    fontWeight: 680
    lineHeight: 1.4
    letterSpacing: "0"
  body:
    fontFamily: '"Segoe UI Variable", "Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif'
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.6
    letterSpacing: "0"
  label:
    fontFamily: '"Segoe UI Variable", "Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif'
    fontSize: "12px"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "0"
rounded:
  control: "5px"
  panel: "7px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  2xl: "32px"
  3xl: "40px"
components:
  button-primary:
    backgroundColor: "{colors.chat}"
    textColor: "{colors.on-accent}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "34px"
  button-secondary:
    backgroundColor: "{colors.raised-surface}"
    textColor: "{colors.text-primary}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "34px"
  activity-item:
    backgroundColor: "{colors.activity-rail}"
    textColor: "{colors.overview}"
    rounded: "6px"
    height: "38px"
    width: "38px"
---

## Overview

**Creative North Star: "The Chromatic Memory Index."** Mnemora keeps the visual logic of an index, but its five workspaces remain visibly color-coded at all times. A dark ink activity rail holds the full spectrum while softly tinted sidebars and active states make context recognizable without turning dense work surfaces into decoration.

**Key Characteristics:**

- A dark ink activity rail with five persistent workspace colors.
- Seven complete multi-color theme presets for different viewing moods.
- Workspace-aware hover, selection, action, source, and progress color.
- Compact navigation and controls around spacious content and reading fields.
- Information-bearing rails, timestamps, source labels, and loading skeletons.

**The Useful Structure Rule.** A divider, index number, color, or rail must communicate location, order, source, status, or progress.

## Colors

The default shell pairs cool lavender surfaces with a deep aubergine activity rail. Forest, Ocean, Rose, Paper, Graphite, and High Contrast are full theme systems rather than monochrome tints. Each preset remaps surfaces and the full workspace spectrum together. Dark variants use composed dark surfaces and brighter workspace colors instead of mechanically inverting light values.

Workspace meaning remains stable: Overview amber, Chat teal, Notes pink or coral, Literature blue, English green, and Settings violet. Exact values adapt to the selected theme. Each has a paired soft surface and a verified foreground for solid controls. Success, warning, danger, and information have independent foreground, soft, and border roles; workspace color never substitutes for status.

**The Workspace Identity Rule.** Show every workspace color in the activity rail, fill the active destination, and carry its color into primary actions, source markers, index rails, selection, and truthful progress. Large reading surfaces stay quiet.

## Typography

The interface uses the Segoe UI Variable stack for an earned Windows desktop feel. UI type stays fixed at 14px. User-controlled reading size applies through `--reading-font-size` to long-form Chat, notes, and reading content without resizing toolbars or stable panels.

Hierarchy is compact: 26px workspace headlines, 16px section titles, 13-14px body and row titles, and 10-12px labels or metadata. Numeric data uses tabular figures. Letter spacing is zero; reading content alone may use the configured reading spacing.

**The Stable Chrome Rule.** Reading preferences may change prose, but must not change navigation, controls, tables, labels, or panel geometry.

## Layout

The shell starts with a fixed 48px activity rail, followed by an optional persisted workspace sidebar, a flexible primary stage, and an optional context panel. The application supports a minimum 720 x 520 window; full multi-panel layouts target 1200px and wider.

Spacing follows 4, 8, 12, 16, 24, 32, and 40px steps. Tool surfaces are dense. Reading and continuation surfaces use constrained measures and larger outer gutters. At 1240px context panels become right overlays; at 940px they take the full stage width. Type sizes do not scale with viewport width.

**The Stable Geometry Rule.** Fixed-format controls, rails, and panels keep explicit dimensions so loading, hover, long labels, and state changes cannot shift the shell.

## Elevation & Depth

Ordinary hierarchy comes from surface contrast and one-pixel borders. Shadows are reserved for menus, composers, dialogs, drawers, and overlay context panels. They use a visible directional offset with a soft blur; normal content sections remain unshadowed.

## Shapes

Controls use restrained 5-6px corners. Framed panels may use 7px. Circular shapes are reserved for point markers, avatars, toggles, and icon states that are intrinsically round. Content sections are not wrapped in decorative cards.

## Components

### Activity Navigation

The activity rail is 48px wide with 38px square icon buttons on a dark ink surface. Every destination keeps a tinted workspace tile; the active item becomes a solid workspace color with a short white rail marker. Tooltips and accessible names identify icons.

### Buttons

Primary commands are 34px high, use the current workspace foreground, and include an icon when it improves scanning. Secondary commands use a raised surface and one-pixel border. Hover changes both surface and boundary; focus uses a two-pixel workspace-colored outline.

### Continuation Index

Overview groups local items chronologically. A one-pixel vertical rail, ordinal, workspace-colored source icon, title, excerpt, source kind, timestamp, and open affordance form each row. Counts remain secondary inline metadata rather than dashboard cards.

### Loading And Empty States

Workspace loading uses a fixed rail-and-page skeleton so lazy module loading does not resize the stage. Reduced-motion mode removes pulsing. Empty and error states state the current fact and expose one recovery or next action.

## Do's and Don'ts

### Do:

- **Do** keep all five workspace identities visible and theme them as one coordinated spectrum.
- **Do** compose light and dark variants independently for every theme preset.
- **Do** keep status meaning paired with text, icons, shape, or position.
- **Do** use Lucide icons, visible focus states, bounded lists, and stable skeleton dimensions.
- **Do** let long-form content breathe while keeping navigation and utilities compact.

### Don't:

- **Don't** build card walls, nested cards, marketing heroes, glass surfaces, or decorative gradients.
- **Don't** use workspace colors as success or failure colors.
- **Don't** let reading font settings resize application chrome.
- **Don't** collapse a theme preset into one hue or return inactive navigation to grayscale.
- **Don't** add decorative animation, gamification, or color without hierarchy or context.
