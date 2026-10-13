import { create } from "zustand";

export type ToastKind = "ok" | "wn" | "er" | "info";

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  detail?: string;
  action?: ToastAction;
}

interface ToastStore {
  toasts: Toast[];
  push: (kind: ToastKind, title: string, detail?: string, action?: ToastAction) => void;
  dismiss: (id: number) => void;
  clear: () => void;
}

let nextId = 1;

export const useToastStore = create<ToastStore>((set) => ({
  toasts: [],
  push: (kind, title, detail, action) =>
    set((state) => ({
      // Keep the stack short so it never covers the interface.
      toasts: [...state.toasts.slice(-3), { id: nextId++, kind, title, detail, action }],
    })),
  dismiss: (id) => set((state) => ({ toasts: state.toasts.filter((toast) => toast.id !== id) })),
  clear: () => set({ toasts: [] }),
}));

/** Convenience helper used across the app. */
export function toast(kind: ToastKind, title: string, detail?: string, action?: ToastAction): void {
  useToastStore.getState().push(kind, title, detail, action);
}

export const toastSuccess = (title: string, detail?: string) => toast("ok", title, detail);
export const toastInfo = (title: string, detail?: string) => toast("info", title, detail);
export const toastWarning = (title: string, detail?: string) => toast("wn", title, detail);
export const toastError = (title: string, detail?: string) => toast("er", title, detail);
