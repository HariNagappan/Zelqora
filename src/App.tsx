import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { HashRouter, Route, Routes } from "react-router-dom";

import Ambient from "./components/Ambient";
import { IconSprite } from "./components/Icon";
import Layout from "./components/Layout";
import Onboarding from "./components/Onboarding";
import Overlay from "./components/Overlay";
import Toasts from "./components/Toasts";
import Activity from "./pages/Activity";
import Home from "./pages/Home";
import Settings from "./pages/Settings";
import Tasks from "./pages/Tasks";
import { subscribeNovaEvents } from "./services/tauri";
import { paintDocument, useAppearanceStore } from "./stores/useAppearance";
import { useNovaStore } from "./stores/useNova";
import { isTauriRuntime } from "./lib/window";

/**
 * Which window this bundle is running in. Reading the Tauri window label is
 * defensive: in a plain browser (UI preview, screenshot tooling) it is absent,
 * and a missing label must never blank the interface.
 */
function currentWindowLabel(): string {
  try {
    return getCurrentWindow().label || "main";
  } catch {
    return "main";
  }
}

export default function App() {
  const label = currentWindowLabel();
  // Outside Tauri the assistant can be reviewed on its own route
  // ("#/overlay?state=listening") — see Overlay's design-review hatch.
  const previewOverlay = !isTauriRuntime() && window.location.hash.startsWith("#/overlay");
  const isOverlay = label === "overlay" || previewOverlay;
  const appearance = useAppearanceStore();
  const appearanceReady = appearance.ready;
  const beginInteraction = useNovaStore((store) => store.beginInteraction);
  const state = useNovaStore((store) => store.state);
  const [reducedMotion, setReducedMotion] = useState(false);

  // Backend events: one subscription per window for its whole lifetime.
  useEffect(() => {
    // `classList.add("")` throws, so each window class is toggled explicitly.
    document.body.classList.toggle("win-overlay", isOverlay);
    document.body.classList.toggle("win-main", !isOverlay);
    const pending = subscribeNovaEvents();
    return () => {
      void pending.then((unlisteners) => unlisteners.forEach((unlisten) => unlisten()));
    };
  }, [isOverlay]);

  useEffect(() => {
    void appearance.init(isOverlay ? "overlay" : "app");
    return appearance.watchSystemTheme();
    // Intentionally once per window: the store keeps the state after that.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) {
      return;
    }
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    setReducedMotion(query.matches);
    const handler = (event: MediaQueryListEvent) => setReducedMotion(event.matches);
    query.addEventListener("change", handler);
    return () => query.removeEventListener("change", handler);
  }, []);

  // A fresh interaction clears the previous transcript/result.
  useEffect(() => {
    if (state === "WAKE_DETECTED") {
      beginInteraction();
    }
  }, [beginInteraction, state]);

  // The window is painted from the *applied* configuration only. A theme being
  // previewed in Settings → Appearance is scoped to the preview panel (see
  // AppearancePane), so nothing about the preview leaks into the application.
  useEffect(() => {
    if (!appearanceReady) {
      return;
    }
    paintDocument(appearance.applied, appearance.systemIsDark, reducedMotion);
  }, [appearance.applied, appearance.systemIsDark, appearanceReady, reducedMotion]);

  const listening = useNovaStore((store) => store.state === "LISTENING_FOR_COMMAND");
  const result = useNovaStore((store) => store.result);
  const [flash, setFlash] = useState(0);

  useEffect(() => {
    if (result?.success) {
      setFlash((value) => value + 1);
    }
  }, [result]);

  return (
    <>
      <IconSprite />
      <Ambient effects={appearance.applied.effects} voiceActive={listening} successFlash={flash} />
      <div className="aur" aria-hidden="true" />
      {isOverlay ? (
        <>
          <Overlay />
          <Toasts />
        </>
      ) : (
        <HashRouter>
          <Layout>
            <Routes>
              <Route
                path="/"
                element={
                  <main className="main pg on">
                    <Home />
                  </main>
                }
              />
              <Route
                path="/tasks"
                element={
                  <main className="main pg on">
                    <Tasks />
                  </main>
                }
              />
              {/* The pre-design route name is kept so existing links and
                  bookmarks (#/todos) keep working. */}
              <Route
                path="/todos"
                element={
                  <main className="main pg on">
                    <Tasks />
                  </main>
                }
              />
              <Route
                path="/activity"
                element={
                  <main className="main pg on">
                    <Activity />
                  </main>
                }
              />
              <Route
                path="/settings"
                element={
                  <main className="main pg on">
                    <Settings />
                  </main>
                }
              />
              <Route
                path="*"
                element={
                  <main className="main pg on">
                    <Home />
                  </main>
                }
              />
            </Routes>
          </Layout>
          <Onboarding />
        </HashRouter>
      )}
    </>
  );
}
