import { create } from "zustand";

import type { ServicePayload } from "../types";

interface ServiceStore {
  /** Latest `nova:service` payload (assistant status + microphone health). */
  payload: ServicePayload | null;
  set: (payload: ServicePayload) => void;
}

export const useServiceStore = create<ServiceStore>((set) => ({
  payload: null,
  set: (payload) => set({ payload }),
}));

export function notifyServiceChange(payload: ServicePayload): void {
  useServiceStore.getState().set(payload);
}
