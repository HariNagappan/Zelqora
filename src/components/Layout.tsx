import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { NavLink, useLocation, useNavigate } from "react-router-dom";

import Toasts from "./Toasts";
import DialogHost from "./Dialog";
import TrayPanel from "./TrayPanel";
import { Icon, type IconName } from "./Icon";
import { Dot } from "./ui";
import { hideWindow, minimizeWindow, toggleMaximizeWindow } from "../lib/window";
import { useAppearanceStore } from "../stores/useAppearance";
import { useNovaStore, type OverlayVisual } from "../stores/useNova";
import { useOnboardingStore } from "../stores/useOnboarding";
import { useStatusStore } from "../stores/useStatus";
import { toast, toastInfo } from "../stores/useToasts";
import { api } from "../services/tauri";
import type { NavigatePayload } from "../types";

const NAV: { to: string; label: string; icon: IconName; test: (path: string) => boolean }[] = [
  { to: "/", label: "Home", icon: "home", test: (path) => path === "/" },
  { to: "/tasks", label: "Tasks", icon: "tasks", test: (path) => path.startsWith("/tasks") },
  { to: "/activity", label: "Activity", icon: "clock", test: (path) => path.startsWith("/activity") },
  { to: "/settings", label: "Settings", icon: "set", test: (path) => path.startsWith("/settings") },
];

export default function Layout({ children }: { children: React.ReactNode }) {
  const location = useLocation();
  const navigate = useNavigate();
  const [trayOpen, setTrayOpen] = useState(false);

  const status = useStatusStore((store) => store.status);
  const refreshStatus = useStatusStore((store) => store.refresh);
  const nova = useNovaStore();
  const behaviourRef = useRef<{ notifications_enabled: boolean } | null>(null);
  const queued = useRef<string[]>([]);
  const appearance = useAppearanceStore();
  const applied = appearance.applied;
  const loadOnboarding = useOnboardingStore((store) => store.load);
  const onboardingState = useOnboardingStore((store) => store.state);
  const openOnboarding = useOnboardingStore((store) => store.setOpen);

  // Status + onboarding are fetched once; lifecycle changes arrive as events.
  useEffect(() => {
    void refreshStatus();
    void loadOnboarding();
  }, [loadOnboarding, refreshStatus]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus, nova.activityBump]);

  // Adopt the backend snapshot once it arrives: the UI must never keep
  // assuming "running" when Nova is paused, stopped or microphone-less.
  useEffect(() => {
    if (!status) {
      return;
    }
    const store = useNovaStore.getState();
    store.setState({
      state: status.state,
      detail: status.detail,
      paused: status.paused,
      service: status.service,
    });
    store.setMicrophone(status.microphone);
    store.setOverlayMode(status.overlayMode);
  }, [status]);

  // Behaviour preferences drive the in-app notification queue below.
  useEffect(() => {
    void api
      .getBehaviour()
      .then((prefs) => {
        behaviourRef.current = prefs;
      })
      .catch(() => undefined);
  }, []);

  // "Show a notification when a command finishes": results that arrive while
  // this window is hidden are surfaced as toasts when you come back. Results
  // you can already see (window visible) never double-notify.
  useEffect(() => {
    if (!nova.result) {
      return;
    }
    if (document.hidden) {
      queued.current.push(nova.result.response);
    }
  }, [nova.result]);

  // A cancelled interaction is reported once, matching the designed toast.
  const lastCancelled = useRef<string | null>(null);
  useEffect(() => {
    if (nova.state !== "CANCELLED") {
      lastCancelled.current = null;
      return;
    }
    const detail = nova.detail ?? "Nothing was changed.";
    if (lastCancelled.current === detail) {
      return;
    }
    lastCancelled.current = detail;
    toast("wn", "Command cancelled", "Nothing was changed.");
  }, [nova.detail, nova.state]);

  useEffect(() => {
    const onVisible = () => {
      if (document.hidden || queued.current.length === 0) {
        return;
      }
      const pending = [...queued.current];
      queued.current = [];
      if (behaviourRef.current?.notifications_enabled === false) {
        return;
      }
      for (const summary of pending.slice(-3)) {
        toastInfo("While you were away", summary);
      }
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, []);

  // First run: the wizard opens automatically until it has been completed.
  useEffect(() => {
    if (onboardingState && !onboardingState.completed) {
      openOnboarding(true);
    }
  }, [onboardingState, openOnboarding]);

  // Tray / single-instance navigation requests from the backend.
  useEffect(() => {
    const pending = listen<NavigatePayload>("nova:navigate", (event) => {
      const page = event.payload.page;
      if (page === "onboarding") {
        openOnboarding(true);
        return;
      }
      const route = page === "home" ? "/" : `/${page}`;
      navigate(route);
    });
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, [navigate, openOnboarding]);

  // Escape closes the quick panel first, like the design's tray behaviour.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setTrayOpen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const service = nova.service;
  const microphone = nova.microphone;
  const visual: OverlayVisual = !microphone.available
    ? "denied"
    : service === "stopped"
      ? "cancelled"
      : service === "paused"
        ? "cancelled"
        : "idle";

  const statusText = () => {
    if (!status) {
      // The backend snapshot hasn't arrived yet — say so instead of guessing.
      return "Checking Nova’s status…";
    }
    if (!microphone.available) {
      return "Microphone unavailable";
    }
    if (service === "stopped") {
      return "Nova is stopped";
    }
    if (service === "paused") {
      return "Listening paused";
    }
    const wakeWord = status.wakeWord || "hey nova";
    if (!status.modelReady) {
      return "Typed commands only — speech model missing";
    }
    return `Listening for “${wakeWord}”`;
  };

  const dotKind = !status
    ? "mut"
    : !microphone.available
      ? "er"
      : service === "running"
        ? "ok"
        : service === "paused"
          ? "wn"
          : "mut";

  const toggleTheme = () => {
    const next = applied.theme === "light" ? "nova" : "light";
    void appearance.applyTheme(next);
  };

  return (
    <div className="app">
      <header className="tbar" data-tauri-drag-region>
        <div className="tl" data-tauri-drag-region>
          <Icon name="nova" />
          <span data-tauri-drag-region>Nova</span>
        </div>
        <div className="wc">
          <button className="ibt" aria-label="Minimize" title="Minimize" onClick={() => void minimizeWindow()}>
            <Icon name="min" />
          </button>
          <button className="ibt" aria-label="Maximize" title="Maximize" onClick={() => void toggleMaximizeWindow()}>
            <Icon name="max" />
          </button>
          <button
            className="ibt cls"
            aria-label="Close window (keeps Nova in the tray)"
            title="Close window (keeps Nova in the tray)"
            onClick={() => {
              void hideWindow();
              toastInfo("Nova is still running", "Wake-word detection continues in the system tray.");
            }}
          >
            <Icon name="x" />
          </button>
        </div>
      </header>

      <div className="win">
        <aside className="side">
          <div className="brand">
            <span className="orb">
              <Icon name="nova" size={18} />
            </span>
            <div>
              <div className="bn">Nova</div>
              <div className="bs">Voice assistant</div>
            </div>
          </div>

          <nav aria-label="Main navigation">
            {NAV.map((item) => {
              const active = item.test(location.pathname);
              return (
                <NavLink
                  key={item.to}
                  to={item.to}
                  className={`ni${active ? " on" : ""}`}
                  aria-current={active ? "page" : undefined}
                  onClick={() => setTrayOpen(false)}
                >
                  <Icon name={item.icon} />
                  <span>{item.label}</span>
                </NavLink>
              );
            })}
          </nav>

          <div className="sfoot">
            <button
              className="statp"
              aria-expanded={trayOpen}
              aria-label="Assistant status and quick controls"
              onClick={() => setTrayOpen((open) => !open)}
            >
              <Dot kind={dotKind} live={Boolean(status) && service === "running" && microphone.available} />
              <span>{statusText()}</span>
            </button>
            <div className="row" style={{ gap: 6 }}>
              <button
                className="ibt"
                id="btnTheme"
                aria-label={applied.theme === "light" ? "Switch to dark theme" : "Switch to light theme"}
                title={applied.theme === "light" ? "Switch to dark theme" : "Switch to light theme"}
                onClick={toggleTheme}
              >
                <Icon name={applied.theme === "light" ? "sun" : "moon"} />
              </button>
              <span className="grow" />
              <span className="dim" style={{ fontSize: 10.5 }}>
                {status?.version ? `v${status.version}` : ""}
              </span>
            </div>
          </div>
        </aside>

        {children}
      </div>

      {trayOpen ? (
        <TrayPanel
          onClose={() => setTrayOpen(false)}
          visual={visual}
          statusText={statusText()}
          dotKind={dotKind}
        />
      ) : null}

      <DialogHost />
      <Toasts />
    </div>
  );
}
