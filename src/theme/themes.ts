// Theme catalogue + token helpers, mirroring the design handoff and the Rust
// `appearance` module. The CSS token sets live in styles.css under
// `[data-th="<id>"]`; this file carries the metadata, the per-theme effect
// presets and the accent maths shared with the backend.

import type { Appearance, Density, Effects, Intensity, ThemeId } from "../types";

export interface ThemeMeta {
  id: ThemeId;
  name: string;
  description: string;
  /** Light themes get slightly reduced ambient opacity in CSS. */
  light: boolean;
}

/** The ten themes from the design handoff, in gallery order. */
export const THEMES: ThemeMeta[] = [
  {
    id: "nova",
    name: "Nova Neon",
    description: "Signature near-black glass with electric cyan and violet energy.",
    light: false,
  },
  {
    id: "darcula",
    name: "Darcula",
    description: "Charcoal IDE surfaces with muted lavender accents — quiet for long sessions.",
    light: false,
  },
  {
    id: "midnight",
    name: "Midnight",
    description: "Deep navy blues with a calm, premium nighttime atmosphere.",
    light: false,
  },
  {
    id: "oled",
    name: "OLED Black",
    description: "Near-pure black with restrained highlights for OLED displays.",
    light: false,
  },
  {
    id: "light",
    name: "Light",
    description: "Clean white and soft gray surfaces with crisp blue accents.",
    light: true,
  },
  {
    id: "nord",
    name: "Nord",
    description: "Nordic blue-grays, icy accents, and soft balanced contrast.",
    light: false,
  },
  {
    id: "dracula",
    name: "Dracula",
    description: "Rich violet nights with pink and cyan sparks.",
    light: false,
  },
  {
    id: "soldark",
    name: "Solarized Dark",
    description: "Warm, low-glare dark with teal and earthy gold.",
    light: false,
  },
  {
    id: "sollight",
    name: "Solarized Light",
    description: "Warm cream light with balanced teal and gold accents.",
    light: true,
  },
  {
    id: "system",
    name: "System Default",
    description: "Follows the Windows app theme automatically.",
    light: false,
  },
];

export const ACCENTS = ["#3fe0ff", "#a78bff", "#5f9bff", "#4beaaf", "#ffc861", "#ff6ad5", "#ff6b8a"];

export const DENSITIES: { id: Density; label: string }[] = [
  { id: "comfortable", label: "Comfortable" },
  { id: "compact", label: "Compact" },
  { id: "spacious", label: "Spacious" },
];

export const INTENSITIES: { id: Intensity; label: string }[] = [
  { id: "minimal", label: "Minimal" },
  { id: "balanced", label: "Balanced" },
  { id: "immersive", label: "Immersive" },
];

export function themeMeta(id: string): ThemeMeta {
  return THEMES.find((theme) => theme.id === id) ?? THEMES[0];
}

export function themeName(id: string): string {
  return themeMeta(id).name;
}

/** `system` resolves to a concrete theme for the Windows app theme. */
export function resolveTheme(id: ThemeId, systemIsDark: boolean): Exclude<ThemeId, "system"> {
  if (id === "system") {
    return systemIsDark ? "nova" : "light";
  }
  return id as Exclude<ThemeId, "system">;
}

/** Effect presets a theme applies when it is chosen (matches the Rust table). */
export function effectsForTheme(id: ThemeId): Effects {
  const resolved = resolveTheme(id, true);
  const base: Record<string, [boolean, boolean, number, Intensity]> = {
    nova: [true, true, 100, "balanced"],
    darcula: [false, false, 25, "minimal"],
    midnight: [true, true, 60, "balanced"],
    oled: [false, true, 20, "minimal"],
    light: [true, true, 35, "balanced"],
    nord: [true, true, 35, "balanced"],
    dracula: [true, true, 70, "balanced"],
    soldark: [false, true, 30, "balanced"],
    sollight: [false, false, 25, "minimal"],
  };
  const [particles, network, glow, intensity] = base[resolved] ?? base.nova;
  return { particles, network, parallax: true, voice: true, glow, transparency: 100, intensity };
}

export const DEFAULT_APPEARANCE: Appearance = {
  theme: "nova",
  accent: null,
  density: "comfortable",
  effects: effectsForTheme("nova"),
  customized: false,
};

/** Motion multiplier per animation intensity (shared with the Rust side). */
export function motionMultiplier(intensity: Intensity): number {
  return intensity === "minimal" ? 0.65 : intensity === "immersive" ? 1.3 : 1;
}

export function ambientSpeed(intensity: Intensity): number {
  return intensity === "minimal" ? 0.7 : intensity === "immersive" ? 1.3 : 1;
}

// ── accent maths (mirrors appearance.rs) ──────────────────────────────────

export function normalizeHex(value: string): string | null {
  const trimmed = value.trim().replace(/^#/, "");
  const expanded =
    trimmed.length === 3
      ? trimmed
          .split("")
          .map((char) => char + char)
          .join("")
      : trimmed;
  if (expanded.length !== 6 || !/^[0-9a-fA-F]{6}$/.test(expanded)) {
    return null;
  }
  return `#${expanded.toLowerCase()}`;
}

function rgb(hex: string): [number, number, number] {
  const normalized = normalizeHex(hex) ?? "#3fe0ff";
  const value = Number.parseInt(normalized.slice(1), 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

export function rgba(hex: string, alpha: number): string {
  const [r, g, b] = rgb(hex);
  return `rgba(${r},${g},${b},${alpha})`;
}

/** Lighten toward white by `factor` (0..1) — used for the hover accent. */
export function mixWithWhite(hex: string, factor: number): string {
  const mixed = rgb(hex).map((channel) =>
    Math.round(channel + (255 - channel) * factor)
      .toString(16)
      .padStart(2, "0"),
  );
  return `#${mixed.join("")}`;
}

/** WCAG relative luminance, used to pick a readable foreground on the accent. */
export function luminance(hex: string): number {
  const channel = (value: number) => {
    const c = value / 255;
    return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  };
  const [r, g, b] = rgb(hex);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

export function onAccent(hex: string): string {
  return luminance(hex) > 0.45 ? "#08131a" : "#ffffff";
}

/**
 * The CSS variables a custom accent overrides. Applied to a scope element so
 * the same code path serves the app, the overlay and the preview container.
 */
export function accentVariables(accent: string): Record<string, string> {
  return {
    "--A": accent,
    "--A2": mixWithWhite(accent, 0.28),
    "--As": rgba(accent, 0.14),
    "--onA": onAccent(accent),
    "--glowC": rgba(accent, 0.3),
    "--glowC2": rgba(accent, 0.45),
    "--glowF": rgba(accent, 0.1),
    "--glowS": rgba(accent, 0.5),
    "--accSoft1": rgba(accent, 0.2),
    "--accSoft2": rgba(accent, 0.07),
    "--accLine": rgba(accent, 0.55),
    "--fx1": accent,
  };
}

/** Scale the alpha channel of an `rgba(...)` string. */
export function alphaScale(value: string, multiplier: number): string {
  const match = value.match(/rgba?\(([^)]+)\)/);
  if (!match) {
    return value;
  }
  const parts = match[1].split(",").map((part) => part.trim());
  if (parts.length < 3) {
    return value;
  }
  const alpha = parts.length >= 4 ? Number.parseFloat(parts[3]) : 1;
  const next = Math.min(1, Math.max(0, alpha * multiplier));
  return `rgba(${parts[0]},${parts[1]},${parts[2]},${next.toFixed(3)})`;
}

const GLOW_VARS = ["--glowC", "--glowC2", "--glowF", "--glowS"];

/**
 * Apply a theme + customisation to an element scope.
 *
 * Tokens come from the CSS `[data-th]` table; a custom accent and the glow /
 * transparency multipliers are layered on top as inline variables. The preview
 * container uses exactly the same function, which is what keeps the preview
 * isolated: nothing here touches application state.
 */
export function applyThemeToElement(
  element: HTMLElement,
  options: {
    theme: ThemeId;
    accent?: string | null;
    effects?: Effects;
    systemIsDark?: boolean;
  },
): void {
  const { theme, accent, effects, systemIsDark = true } = options;
  element.setAttribute("data-th", resolveTheme(theme, systemIsDark));

  for (const name of ["--A", "--A2", "--As", "--onA", ...GLOW_VARS, "--accSoft1", "--accSoft2", "--accLine", "--fx1"]) {
    element.style.removeProperty(name);
  }

  if (accent) {
    const normalized = normalizeHex(accent);
    if (normalized) {
      for (const [name, value] of Object.entries(accentVariables(normalized))) {
        element.style.setProperty(name, value);
      }
    }
  }

  if (!effects) {
    return;
  }
  const glowMultiplier = Math.max(0.05, effects.glow / 100);
  const transparencyMultiplier = Math.max(0.5, effects.transparency / 100);
  const computed = getComputedStyle(element);
  if (glowMultiplier !== 1) {
    for (const name of GLOW_VARS) {
      const base = computed.getPropertyValue(name).trim();
      if (base) {
        element.style.setProperty(name, alphaScale(base, glowMultiplier));
      }
    }
  }
  if (transparencyMultiplier !== 1) {
    const base = computed.getPropertyValue("--glass").trim();
    if (base) {
      element.style.setProperty("--glass", alphaScale(base, transparencyMultiplier));
    }
  }
}
