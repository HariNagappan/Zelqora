# Appearance system — implementation handoff

This documents the Appearance feature as implemented, including the token
tables, the state model, preview isolation, persistence and Windows
integration. It is the handoff the design asked for, written against the code
that actually ships.

## 1. Token tables

Semantic tokens are CSS custom properties. Two shapes exist:

- **Theme tables** live in `src/styles.css` under `[data-th="<theme-id>"]`
  (10 themes). Any element — `<body>` for the applied theme, a preview
  container, or a gallery card — becomes a theme scope by carrying that
  attribute. Nothing in a component hardcodes a colour.
- **Per-user multipliers** (`accent`, `glow`, `transparency`, `density`,
  motion) are layered on top as inline variables by
  `applyThemeToElement()` in `src/theme/themes.ts`.

| Token | Meaning |
| --- | --- |
| `--bg0` / `--bg1` | window background, chrome (title bar, sidebar) |
| `--bg2` / `--bg3` | surface, elevated surface / input background |
| `--glass` | translucent panel colour (`--panelOpacity` equivalent) |
| `--fg`, `--fg2`, `--fg3` | primary, secondary, tertiary text |
| `--bd`, `--bd2` | hairline border, emphasised border |
| `--A`, `--A2`, `--As`, `--onA` | accent, accent hover, accent soft fill, readable text on accent |
| `--accLine`, `--accSoft1/2` | accent strokes and gradients for selected states |
| `--ok`, `--wn`, `--er` (+ `--okS`, `--wnS`, `--erS`) | status colours and their soft fills |
| `--glowC`, `--glowC2`, `--glowF`, `--glowS` | glow strengths (button, hover, fill, strong) |
| `--sh1`, `--sh2`, `--sh3` | elevation levels |
| `--edg` | inner top highlight used on glass surfaces |
| `--sideA/--sideB`, `--barC`, `--voA/--voB` | chrome and overlay gradients |
| `--aur1`, `--aur2`, `--fx1`, `--fx2` | ambient aurora + canvas colours |
| `--M` | motion multiplier (0 / 0.65 / 1 / 1.3) applied to every transition and keyframe |

`appearance.rs` owns the same catalogue in Rust (ids, names, descriptions,
light/dark flag, per-theme effect presets, accent luminance/contrast helpers), so
the backend can validate what the UI sends.

## 2. Theme catalogue and presets

Ten themes, in gallery order: Nova Neon, Darcula, Midnight, OLED Black, Light,
Nord, Dracula, Solarized Dark, Solarized Light, System Default.
Darcula (charcoal + muted lavender, glow 25 %, animations minimal) and Dracula
(violet/pink/cyan, glow 70 %) are deliberately distinct.

Applying a theme adopts that theme’s preset for the background effects
(`Effects::for_theme` in Rust, `effectsForTheme` in TS — kept identical) and
drops a custom accent; the presets are also what “Reset” restores.

`System Default` resolves through `matchMedia("(prefers-color-scheme: dark)")`
(the value Windows exposes to WebView2) to `nova` or `light`. A listener updates
the app live when Windows switches, and a manually chosen theme stays fixed.
Only Nova’s own surfaces follow the system setting — the rest of Windows is
outside Nova’s control, and the UI says so.

## 3. State model

```
applied    Appearance   ← persisted (SQLite row `appearance`)
preview    Appearance?  ← temporary, frontend-only, never persisted
systemIsDark boolean    ← Windows app theme
```

`useAppearance` (zustand) owns all three. `activeAppearance()` returns the
preview when one is open, otherwise the applied configuration.

Rules enforced by the store:

- **Preview never writes.** `startPreview`, `setAccent`, `setDensity`,
  `setEffect` and `setIntensity` mutate `preview` only.
- **Apply commits.** `applyPreview()` / `applyTheme()` call
  `update_appearance`, then adopt the normalized result the backend returns as
  `applied`.
- **Cancel restores.** `cancelPreview()` drops the preview object; the app
  immediately re-renders from `applied`.
- **Reset** restores the theme presets (accent cleared, density comfortable,
  effect defaults) without changing the theme itself, while
  “Reset appearance to defaults” (confirmation dialog) returns to Nova Neon.
- **Customized** is set as soon as the user changes accent, density or an
  effect, and is surfaced as a `Customized` tag next to the applied theme.

## 4. Preview isolation

The preview is a **scope, not a mode**:

- All preview controls write to `preview` in the store; nothing reads it except
  the preview panel and the appearance-aware `Ambient` canvas props.
- The preview panel carries `data-th` + inline accent/glow/transparency
  variables set by the same `applyThemeToElement()` used for the app.
- Because the preview is pure CSS scoping, nothing inside it can create a task,
  change a setting, start recognition or run a command. The panel’s interactive
  elements are decorative by construction (spans, disabled inputs, buttons
  marked `aria-hidden`).

## 5. Persistence

- `update_appearance` normalizes and validates the payload
  (`theme` must exist, accent must be `#rgb`/`#rrggbb`, glow 0–150,
  transparency 60–100), stores it as JSON in the `settings` table under
  `appearance`, and broadcasts `nova:appearance`.
- The overlay window subscribes to the same event, so both windows always show
  the same theme; a window that loads later reads `get_appearance`.
- Density, motion multiplier and the ambient effect switches are applied to
  `document.body` (`data-density`, `--M`, `data-motion`) — one place, both
  windows.
- Reduced motion wins over everything: the OS setting forces a static ambient
  frame and disables transitions regardless of the chosen intensity.

## 6. Windows integration

- System theme following reads the Windows app theme; Nova never writes it.
- Manual theme selection is annotated as fixed, and System Default is annotated
  as system-controlled, so the two states are never confused.
- Unavailable effects degrade gracefully: if `backdrop-filter` is unsupported
  the surfaces fall back to opaque token colours, and the UI never promises
  effects it cannot render.

## 7. Accessibility

- Text keeps its contrast because accents only drive fills, borders and glows;
  `onAccent()` (WCAG luminance) picks black or white for text on the accent.
- Every control has a visible focus ring (`:focus-visible`), tooltips for
  icon-only buttons, and text labels for every state that is also colour-coded.
- `Segmented`/`Toggle`/`Checkbox` expose `aria-pressed`/`aria-checked`, the
  gallery cards are keyboard-reachable (`Enter`/`Space` preview), and dialogs
  move focus to the affirmative action and restore it on close.
