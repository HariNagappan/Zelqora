import { create } from "zustand";

import { api } from "../services/tauri";
import type { Onboarding } from "../types";

interface OnboardingStore {
  state: Onboarding | null;
  open: boolean;
  error: string | null;
  load: () => Promise<void>;
  setOpen: (open: boolean) => void;
  /** Record a single step fact (mic verified, first command done, …). */
  patch: (patch: Partial<Onboarding>) => Promise<void>;
  finish: () => Promise<void>;
  reset: () => Promise<void>;
}

const fresh: Onboarding = {
  completed: false,
  completed_at: null,
  microphone_verified: false,
  first_command_done: false,
  providers_skipped: false,
};

export const useOnboardingStore = create<OnboardingStore>((set, get) => ({
  state: null,
  open: false,
  error: null,

  load: async () => {
    try {
      const state = await api.getOnboarding();
      set({ state, error: null });
    } catch (error) {
      set({ state: fresh, error: String(error) });
    }
  },

  setOpen: (open) => set({ open }),

  patch: async (patch) => {
    const current = get().state ?? fresh;
    const next = { ...current, ...patch };
    set({ state: next });
    try {
      const saved = await api.updateOnboarding(next);
      set({ state: saved, error: null });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  finish: async () => {
    const current = get().state ?? fresh;
    const next = { ...current, completed: true, completed_at: new Date().toISOString() };
    set({ state: next, open: false });
    try {
      const saved = await api.updateOnboarding(next);
      set({ state: saved, error: null });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  reset: async () => {
    try {
      const saved = await api.resetOnboarding();
      set({ state: saved, open: true, error: null });
    } catch (error) {
      set({ error: String(error) });
    }
  },
}));
