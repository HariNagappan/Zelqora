// Window helpers. Every call is permission-checked by Tauri's capability
// system (see src-tauri/capabilities/default.json) and fails soft: a rejected
// window call must never break the UI.

import { getCurrentWindow } from "@tauri-apps/api/window";

/** True when the bundle runs inside the Tauri shell (not a plain browser). */
export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function currentWindow() {
  return getCurrentWindow();
}

export async function minimizeWindow(): Promise<void> {
  try {
    await getCurrentWindow().minimize();
  } catch (error) {
    console.warn("minimize failed", error);
  }
}

export async function toggleMaximizeWindow(): Promise<void> {
  try {
    const window = getCurrentWindow();
    if (await window.isMaximized()) {
      await window.unmaximize();
    } else {
      await window.maximize();
    }
  } catch (error) {
    console.warn("maximize failed", error);
  }
}

/** Closing the window hides it — Nova keeps running in the tray. */
export async function hideWindow(): Promise<void> {
  try {
    await getCurrentWindow().hide();
  } catch (error) {
    console.warn("hide failed", error);
  }
}

export async function showWindow(): Promise<void> {
  try {
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
  } catch (error) {
    console.warn("show failed", error);
  }
}
