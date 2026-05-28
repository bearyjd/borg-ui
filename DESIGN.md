# Design System — BorgUI

The canonical source for visual and interaction choices. Code lives in
`app-tauri/src/app.css`; this document explains the why. When in doubt, the
CSS variable wins — if you find yourself reaching for a literal `oklch()` or
hex value, add a token here first.

## Product Context

Native Windows GUI for BorgBackup. Users are technical (SSH, repos, retention
policies, exclude patterns) but want a tool that gets out of their way. The app
runs primarily in the system tray; users open it to configure, monitor, or
recover — not to live in it. Surfaces should be dense enough to convey real
information at a glance and quiet enough to disappear when minimized.

## Aesthetic Direction

- **Direction:** Utilitarian dark — function-first, monospace accents where data
  matters, no decorative gradients or hero illustrations.
- **Decoration:** Minimal. Borders and surface elevation do the work; no
  texture, no grain, no animation beyond state transitions.
- **Mood:** Looks like a sysadmin tool that respects your time. Closer to
  Tailscale/Linear than to a SaaS marketing page.
- **Reference points:** Vorta (functional parity, less austere); Tailscale macOS
  app (tray-resident dark UI); native Windows Terminal (monospace + token
  discipline).

## Color (OKLCH)

All colors use OKLCH for perceptual uniformity. The 260° hue is a cool grey
that mixes neutrally with the 165° teal accent. Defined in
`app-tauri/src/app.css`.

### Surface

| Token | Value | Use |
|---|---|---|
| `--color-bg` | `oklch(14% 0.01 260)` | Page background |
| `--color-surface` | `oklch(18% 0.012 260)` | Cards, rows, modals |
| `--color-surface-hover` | `oklch(22% 0.015 260)` | Hover state on surfaces |
| `--color-surface-active` | `oklch(25% 0.015 260)` | Pressed/selected state |
| `--color-border` | `oklch(28% 0.01 260)` | Primary borders |
| `--color-border-subtle` | `oklch(22% 0.008 260)` | Row separators, inset rules |

### Text

| Token | Value | Use |
|---|---|---|
| `--color-text` | `oklch(92% 0.01 260)` | Primary text |
| `--color-text-muted` | `oklch(62% 0.01 260)` | Secondary labels, subtitles |
| `--color-text-dim` | `oklch(45% 0.01 260)` | Tertiary text, timestamps, hints |

### Accent (teal-green, 165°)

| Token | Value | Use |
|---|---|---|
| `--color-accent` | `oklch(72% 0.18 165)` | Primary actions, links, focus |
| `--color-accent-hover` | `oklch(78% 0.18 165)` | Accent button hover |
| `--color-accent-muted` | `oklch(72% 0.18 165 / 0.15)` | Subtle tint backgrounds |
| `--color-on-accent` | `oklch(14% 0 0)` | Text on solid accent backgrounds (near-black) |

### Semantic

Use the muted variants for backgrounds; reserve the solid colors for icons,
borders, and text where contrast is needed.

| Token | Value | Use |
|---|---|---|
| `--color-success` | `oklch(72% 0.16 145)` | Success text, icons |
| `--color-success-muted` | `oklch(75% 0.15 145 / 0.15)` | Success banner backgrounds |
| `--color-warning` | `oklch(78% 0.16 80)` | Warning text, icons |
| `--color-warning-muted` | `oklch(78% 0.16 80 / 0.15)` | Warning banner backgrounds |
| `--color-danger` | `oklch(65% 0.2 25)` | Destructive text, icons, borders |
| `--color-danger-muted` | `oklch(65% 0.2 25 / 0.15)` | Destructive banner / delete button hover |
| `--color-danger-hover` | `oklch(60% 0.22 25)` | Destructive button hover state |

### Overlay

| Token | Value | Use |
|---|---|---|
| `--color-backdrop` | `oklch(0% 0 0 / 0.5)` | Modal scrim |

## Typography

System-friendly stacks; no remote font loading (Tauri app, offline-tolerant).

| Token | Value | Use |
|---|---|---|
| `--font-sans` | `'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif` | All UI text |
| `--font-mono` | `'JetBrains Mono', 'Cascadia Code', 'Fira Code', monospace` | Archive names, paths, file lists, code, repo URLs |

Use `--font-mono` whenever showing data the user might copy/paste: SSH URLs,
file paths, archive names, repo IDs, terminal output.

### Scale

| Token | Size | Use |
|---|---|---|
| `--text-xs` | `0.75rem` (12px) | Captions, badges, sub-labels |
| `--text-sm` | `0.8125rem` (13px) | Default UI text, button labels |
| `--text-base` | `0.875rem` (14px) | Body, paragraphs (root font-size) |
| `--text-lg` | `1rem` (16px) | Modal headings, emphasized text |
| `--text-xl` | `1.25rem` (20px) | Section headings |
| `--text-2xl` | `1.5rem` (24px) | Subpage titles |
| `--text-3xl` | `2rem` (32px) | Page titles (`<h1>` in page-header) |

Headings are visually distinguished by **size and weight**, not color. Use
`font-weight: 700` for `<h1>` and `font-weight: 600` for `<h2>`. Letter-spacing
on large headings: `-0.02em` to `-0.03em`.

## Spacing

4-pixel base unit. Density is **comfortable** — tighter than marketing pages,
roomier than enterprise data grids.

| Token | Value | Common use |
|---|---|---|
| `--space-1` | `0.25rem` (4px) | Icon ↔ label gap |
| `--space-2` | `0.5rem` (8px) | Button padding y, small gaps |
| `--space-3` | `0.75rem` (12px) | Row padding, label spacing |
| `--space-4` | `1rem` (16px) | Standard surface padding, gap between cards |
| `--space-6` | `1.5rem` (24px) | Modal padding, section gaps |
| `--space-8` | `2rem` (32px) | Page-header bottom margin, large section breaks |
| `--space-12` | `3rem` (48px) | Hero spacing (rare) |

Sub-token sizes (`1px`, `2px`) are acceptable inline for thin separators and
micro-adjustments; don't tokenize them.

## Radii

| Token | Value | Use |
|---|---|---|
| `--radius-sm` | `6px` | Inline tags, code chips |
| `--radius-md` | `8px` | Buttons, banners, rows, surfaces |
| `--radius-lg` | `12px` | Modals, large panels |

`border-radius: 50%` is the canonical circle (avatars, status dots) — no token
needed.

## Motion

Minimal-functional. Only animate `background`, `color`, `border-color`,
`opacity`, and `transform`.

| Token | Value | Use |
|---|---|---|
| `--duration-fast` | `120ms` | Button hover, focus rings, micro-states |
| `--duration-normal` | `200ms` | Modal fade, panel slide |
| `--ease-out` | `cubic-bezier(0.16, 1, 0.3, 1)` | Default easing |

Never animate layout-bound properties (width, height, top, left, margin) —
they trigger layout thrash on every frame.

## Components

### Buttons (canonical, defined globally in `app.css`)

Apply `.btn` plus one variant class. All variants share the same shape; the
fill changes.

| Class | Use | Visual |
|---|---|---|
| `.btn .btn-primary` | Default CTA | Solid accent, near-black text |
| `.btn .btn-secondary` | Cancel / refresh / non-primary actions | Surface-hover fill, muted text, subtle border |
| `.btn .btn-restore` | Restore archive (accent-muted alias) | Accent tint fill, accent text, solidifies on hover |
| `.btn .btn-delete` | Delete archive (destructive, low-priority) | Transparent, dim text, danger-tint on hover |
| `.btn .btn-delete-confirm` | Confirmed delete in modal | Solid danger fill |

Disabled buttons get `opacity: 0.5; cursor: not-allowed;` via the global
`.btn:disabled` rule — no per-variant override needed.

### Banners

Pair a muted background with the matching solid color for text:

```css
.error-banner   { background: var(--color-danger-muted);  color: var(--color-danger);  }
.warning-banner { background: var(--color-warning-muted); color: var(--color-warning); }
.success-banner { background: var(--color-success-muted); color: var(--color-success); }
```

Padding: `var(--space-3) var(--space-4)`. Radius: `var(--radius-md)`. Size:
`var(--text-sm)`.

### Modals

```css
.modal-backdrop { background: var(--color-backdrop); }
.modal {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  padding: var(--space-6);
}
```

Modals trap focus on open, close on Escape, and dismiss when clicking the
backdrop. The destructive-confirm modal pattern (used in Delete Archive) is the
reference implementation — copy it for new confirms.

### Page header

Every route opens with the same structure:

```html
<header class="page-header">
  <div class="header-row">
    <div>
      <h1>Title</h1>
      <p class="subtitle">One-line description.</p>
    </div>
    <!-- optional: top-right action button -->
  </div>
</header>
```

`margin-bottom: var(--space-8)` on the header.

### Empty states

Dashed border, muted text, centered:

```css
.empty-state {
  background: var(--color-surface);
  border: 1px dashed var(--color-border);
  border-radius: var(--radius-lg);
  padding: var(--space-8);
  text-align: center;
  color: var(--color-text-muted);
}
```

Always include a CTA link to the next step (e.g. "configure your repository").

## Rules for new code

1. **No hardcoded color values.** Every `oklch()`, `rgb()`, or hex literal in a
   page or component is a bug. Use a token; if no token fits, add one here
   first.
2. **Use the global button variants.** Don't redefine `.btn-primary` in a route;
   override only when the route genuinely needs a new variant (which should be
   rare — add it globally instead).
3. **Use `--font-mono` for copyable strings.** SSH URLs, paths, archive
   identifiers, terminal output.
4. **Spacing tokens for ≥ 4px.** `1px`/`2px` inline is fine for separators.
5. **Animate only compositor-friendly properties.** Transform and opacity move
   on the GPU; layout properties don't.
6. **Headings carry hierarchy.** Don't use color to make text "heading-y" — use
   size and weight.

## Decisions Log

| Date | Decision | Rationale |
|---|---|---|
| 2026-05-28 | Initial DESIGN.md | Codified the existing token system; tokenized 19 inline colors. |
| 2026-05-28 | Promoted `.btn-*` variants to global | Avoid drift between routes that need the same button shape. |
