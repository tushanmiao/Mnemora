---
name: Mnemora
description: A neutral memory-index workstation with fixed workspace identity colors.
colors:
  app-ground: "#f3f5f4"
  content-surface: "#fafbfa"
  raised-surface: "#ffffff"
  sidebar-surface: "#e9eeec"
  text-primary: "#1d2420"
  text-secondary: "#4b5650"
  text-muted: "#6b7770"
  border-default: "#d1d8d4"
  overview: "#9a6500"
  chat: "#0d6b5d"
  notes: "#b5473a"
  literature: "#2864b0"
  english: "#347a3b"
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
    textColor: "{colors.raised-surface}"
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
    backgroundColor: "{colors.sidebar-surface}"
    textColor: "{colors.text-muted}"
    rounded: "6px"
    height: "38px"
    width: "38px"
---

## Overview

**Creative North Star: "The Memory Index Workstation."** Mnemora uses the visual logic of an index: narrow rails locate work, ordered rows preserve chronology, and workspace colors identify context without tinting whole pages. The surrounding shell stays quiet so conversation, notes, PDFs, and study material can carry the user's attention.

**Key Characteristics:**

- Neutral mineral surfaces with clear one-pixel boundaries.
- Five stable workspace colors used for location, action, source, and real progress.
- Compact navigation and controls around spacious content and reading fields.
- Information-bearing rails, timestamps, source labels, and loading skeletons.

**The Useful Structure Rule.** A divider, index number, color, or rail must communicate location, order, source, status, or progress.

## Colors

The default shell is a cool neutral light system. Dark mode uses deep green-black surfaces rather than pure black. Paper and high-contrast presets replace surface and text roles while preserving workspace identity colors.

Workspace color mapping is permanent: Overview amber, Chat teal, Notes coral, Literature blue, and English green. Each has a paired soft surface. Success, warning, danger, and information have independent foreground, soft, and border roles; workspace color never substitutes for status.

**The Workspace Identity Rule.** Use the active workspace color for the current navigation item, its primary action, source markers, index rails, and truthful progress. Do not spread it across the page background.

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

The activity rail is 48px wide with 38px square icon buttons. Inactive items remain neutral; hover and active states reveal that workspace's stable color and soft surface. The active item also carries a short physical rail marker, while tooltips and accessible names identify icons.

### Buttons

Primary commands are 34px high, use the current workspace foreground, and include an icon when it improves scanning. Secondary commands use a raised surface and one-pixel border. Hover changes both surface and boundary; focus uses a two-pixel workspace-colored outline.

### Continuation Index

Overview groups local items chronologically. A one-pixel vertical rail, ordinal, workspace-colored source icon, title, excerpt, source kind, timestamp, and open affordance form each row. Counts remain secondary inline metadata rather than dashboard cards.

### Loading And Empty States

Workspace loading uses a fixed rail-and-page skeleton so lazy module loading does not resize the stage. Reduced-motion mode removes pulsing. Empty and error states state the current fact and expose one recovery or next action.

## Do's and Don'ts

### Do:

- **Do** preserve the neutral shell and fixed five-color workspace mapping.
- **Do** keep status meaning paired with text, icons, shape, or position.
- **Do** use Lucide icons, visible focus states, bounded lists, and stable skeleton dimensions.
- **Do** let long-form content breathe while keeping navigation and utilities compact.

### Don't:

- **Don't** build card walls, nested cards, marketing heroes, glass surfaces, or decorative gradients.
- **Don't** use workspace colors as success or failure colors.
- **Don't** let reading font settings resize application chrome.
- **Don't** add decorative animation, gamification, or color without information.
