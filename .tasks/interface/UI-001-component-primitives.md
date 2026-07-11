---
id: UI-001
title: Core Component Primitives
status: In Progress
assignee: jamiepine
parent: UI-000
priority: High
tags: [ui, components, design-system]
whitepaper: N/A
last_updated: 2026-07-11
---

## Description

Build a complete set of reusable UI primitives in @sd/ui that follow the V2 design system. These are platform-agnostic, accessible components used throughout the interface.

## Components

**Completed:**
- DropdownMenu (expanding, not overlay)
- Button variants

**In Progress:**
- Dialog/Modal
- Tooltip
- Input variants
- Checkbox/Radio
- Slider

**Planned:**
- Tabs
- Accordion
- Select
- Combobox
- Progress
- Badge
- Avatar

## Implementation Notes

- Built on Radix UI for accessibility
- Minimal styling in primitives (styled via className prop)
- Semantic color system support
- Framer Motion for animations
- Platform-agnostic (works on all platforms)

## Location correction

The primitives are **not** in `@sd/ui` — they live in the separate **`spaceui`** repository
(`spaceui/packages/primitives`, published as `@spacedrive/primitives`) and are aliased in
`apps/tauri/vite.config.ts` for local HMR. Work for this task lands in that repo, not the
spacedrive monorepo. This file tracks status; implementation/PR is in spaceui.

## Acceptance Criteria

- [x] DropdownMenu with expanding animation (`DropdownMenu.tsx`)
- [x] Button with variants (primary, secondary, danger) (`Button.tsx` + variants)
- [x] Dialog with backdrop blur (added `backdrop-blur-sm` to both the dialogManager overlay and the composable `DialogOverlay`)
- [x] Tooltip with proper positioning (`Tooltip.tsx` — `position`/`side`/`align`/`sideOffset`)
- [x] Input with validation states (`Input.tsx` — `error` variant, `variant`, `size`)
- [x] Form components (checkbox, radio, slider) (`Checkbox.tsx`, `RadioGroup.tsx`, `Slider.tsx`, `Switch.tsx`)
- [x] All components keyboard accessible (built on Radix primitives)
- [x] All components follow V2 rounded style (`rounded-lg`/`rounded-xl`)
- [ ] Documented in Storybook or docs (Storybook configured; stories added for Button, Input, CheckBox, Slider — remaining primitives still need stories)

## Implementation Notes (2026-07-11)

- Verified the primitives set in `spaceui/packages/primitives` already covers every planned
  component (Dialog, Tooltip, Input, Checkbox, RadioGroup, Slider, Switch, Tabs, Select, Badge,
  ProgressBar, …) — the "In Progress / Planned" lists above were stale.
- Added the one genuinely-missing behavior: **Dialog backdrop blur** (`backdrop-blur-sm` on both
  overlays in `Dialog.tsx`).
- Added Storybook stories (`Input.stories.tsx`, `Checkbox.stories.tsx`, `Slider.stories.tsx`) to
  begin closing the documentation gap (Button already had one). `tsc --noEmit` passes.
- Remaining for full closure: Storybook stories for the rest of the primitives (Tooltip,
  RadioGroup, Dialog, Select, Tabs, …).
