import { create } from "zustand";

import { api } from "../services/tauri";
import {
  DEFAULT_APPEARANCE,
  applyThemeToElement,
  effectsForTheme,
  motionMultiplier,
  normalizeHex,
} from "../theme/themes";
import type { Appearance, Density, Effects, Intensity, ThemeId } from "../types";

type ThemeScope = "app" | "overlay";

interface AppearanceStore {
  /** The committed configuration (persisted by the backend). */
  applied: Appearance;
  /**
   * A temporary configuration being previewed in Settings → Appearance.
   * Preview state is entirely separate from `applied`: nothing here is
   * persisted, and no other part of the app reads it.
   */
  preview: Appearance | null;
  /** Windows app theme, which `system` follows. */
  systemIsDark: boolean;
  /** True once the applied configuration has been loaded. */
  ready: boolean;
  /** Set when the last save failed, so the UI can report it. */
  error: string | null;

  init: (scope: ThemeScope) => Promise<void>;
  receiveApplied: (appearance: Appearance) => void;
  watchSystemTheme: () => () => void;

  // preview lifecycle
  startPreview: (theme: ThemeId) => void;
  updatePreview: (patch: Partial<Appearance>) => void;
  cancelPreview: () => void;
  applyPreview: () => Promise<void>;
  applyTheme: (theme: ThemeId) => Promise<void>;

  // customisation (applies to the preview when one is open, otherwise commits)
  setAccent: (accent: string | null, options?: { previewOnly?: boolean }) => void;
  setDensity: (density: Density, options?: { previewOnly?: boolean }) => void;
  setEffect: <K extends keyof Effects>(key: K, value: Effects[K], options?: { previewOnly?: boolean }) => void;
  setIntensity: (intensity: Intensity, options?: { previewOnly?: boolean }) => void;
  resetCustomization: () => void;
  resetAll: () => Promise<void>;
}

function customized(base: Appearance, patch: Partial<Appearance>): Appearance {
  return { ...base, ...patch, customized: true };
}

async function persist(appearance: Appearance): Promise<Appearance> {
  return api.updateAppearance(appearance);
}

export const useAppearanceStore = create<AppearanceStore>((set, get) => ({
  applied: DEFAULT_APPEARANCE,
  preview: null,
  systemIsDark: true,
  ready: false,
  error: null,

  init: async (scope) => {
    if (typeof window !== "undefined" && window.matchMedia) {
      set({ systemIsDark: window.matchMedia("(prefers-color-scheme: dark)").matches });
    }
    try {
      const applied = await api.getAppearance();
      set({ applied, ready: true, error: null });
      if (scope === "app") {
        document.body.dataset.density = applied.density;
      }
    } catch (error) {
      // The backend is unreachable (window opened outside Tauri, or a failed
      // load): fall back to the defaults so the UI is still usable.
      set({ applied: DEFAULT_APPEARANCE, ready: true, error: String(error) });
    }
  },

  receiveApplied: (appearance) => set({ applied: appearance }),

  watchSystemTheme: () => {
    if (typeof window === "undefined" || !window.matchMedia) {
      return () => {};
    }
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = (event: MediaQueryListEvent) => {
      set({ systemIsDark: event.matches });
    };
    query.addEventListener("change", handler);
    return () => query.removeEventListener("change", handler);
  },

  startPreview: (theme) =>
    set((state) => ({
      preview: {
        theme,
        accent: null,
        density: state.applied.density,
        effects: effectsForTheme(theme),
        customized: false,
      },
    })),

  updatePreview: (patch) =>
    set((state) => ({ preview: state.preview ? { ...state.preview, ...patch } : state.preview })),

  cancelPreview: () => set({ preview: null, error: null }),

  applyPreview: async () => {
    const preview = get().preview;
    if (!preview) {
      return;
    }
    try {
      const applied = await persist(preview);
      set({ applied, preview: null, error: null });
      document.body.dataset.density = applied.density;
    } catch (error) {
      set({ error: String(error) });
    }
  },

  applyTheme: async (theme) => {
    try {
      const applied = await persist({
        theme,
        accent: null,
        density: get().applied.density,
        effects: effectsForTheme(theme),
        customized: false,
      });
      set({ applied, preview: null, error: null });
      document.body.dataset.density = applied.density;
    } catch (error) {
      set({ error: String(error) });
    }
  },

  setAccent: (accent, options) => {
    const normalized = accent ? normalizeHex(accent) : null;
    const state = get();
    if (options?.previewOnly || state.preview) {
      set({
        preview: state.preview
          ? customized(state.preview, { accent: normalized })
          : { ...state.applied, accent: normalized, customized: true },
      });
      return;
    }
    const next = customized(state.applied, { accent: normalized });
    set({ applied: next });
    void persist(next)
      .then((applied) => set({ applied, error: null }))
      .catch((error) => set({ error: String(error) }));
  },

  setDensity: (density, options) => {
    const state = get();
    if (options?.previewOnly || state.preview) {
      set({
        preview: state.preview
          ? customized(state.preview, { density })
          : { ...state.applied, density, customized: true },
      });
      return;
    }
    const next = customized(state.applied, { density });
    document.body.dataset.density = density;
    set({ applied: next });
    void persist(next)
      .then((applied) => set({ applied, error: null }))
      .catch((error) => set({ error: String(error) }));
  },

  setEffect: (key, value, options) => {
    const state = get();
    const withEffect = (base: Appearance): Appearance => ({
      ...base,
      customized: true,
      effects: { ...base.effects, [key]: value },
    });
    if (options?.previewOnly || state.preview) {
      set({
        preview: state.preview ? withEffect(state.preview) : withEffect(state.applied),
      });
      return;
    }
    const next = withEffect(state.applied);
    set({ applied: next });
    void persist(next)
      .then((applied) => set({ applied, error: null }))
      .catch((error) => set({ error: String(error) }));
  },

  setIntensity: (intensity, options) => {
    get().setEffect("intensity", intensity, options);
  },

  resetCustomization: () => {
    const state = get();
    if (state.preview) {
      set({
        preview: {
          ...state.preview,
          accent: null,
          density: "comfortable",
          effects: effectsForTheme(state.preview.theme),
          customized: false,
        },
      });
      return;
    }
    const reset: Appearance = {
      theme: state.applied.theme,
      accent: null,
      density: "comfortable",
      effects: effectsForTheme(state.applied.theme),
      customized: false,
    };
    set({ applied: reset });
    document.body.dataset.density = reset.density;
    void persist(reset)
      .then((applied) => set({ applied, error: null }))
      .catch((error) => set({ error: String(error) }));
  },

  resetAll: async () => {
    try {
      const applied = await api.resetAppearance();
      set({ applied, preview: null, error: null });
      document.body.dataset.density = applied.density;
    } catch (error) {
      set({ error: String(error) });
    }
  },
}));

/** The appearance currently on screen (preview when open, otherwise applied). */
export function activeAppearance(state: {
  applied: Appearance;
  preview: Appearance | null;
}): Appearance {
  return state.preview ?? state.applied;
}

/**
 * Apply the active appearance to the document. Called by the app shell and the
 * overlay wrapper after every store change.
 */
export function paintDocument(
  appearance: Appearance,
  systemIsDark: boolean,
  motionOff: boolean,
): void {
  applyThemeToElement(document.body, {
    theme: appearance.theme,
    accent: appearance.accent,
    effects: appearance.effects,
    systemIsDark,
  });
  document.body.dataset.density = appearance.density;
  const multiplier = motionOff ? 0 : motionMultiplier(appearance.effects.intensity);
  document.body.style.setProperty("--M", String(multiplier));
  document.body.dataset.motion = motionOff ? "off" : "on";
}
