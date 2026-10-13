import { create } from "zustand";

import { api } from "../services/tauri";
import type { StatusInfo } from "../types";

interface StatusStore {
  status: StatusInfo | null;
  error: string | null;
  refresh: () => Promise<void>;
}

/**
 * Backend status (model ready, provider configured, service state, version).
 * Refreshed on demand — the lifecycle states themselves arrive as events, so
 * there is no polling loop.
 */
export const useStatusStore = create<StatusStore>((set) => ({
  status: null,
  error: null,
  refresh: async () => {
    try {
      const status = await api.getStatus();
      set({ status, error: null });
    } catch (error) {
      set({ error: String(error) });
    }
  },
}));
