import { useNavigate } from "react-router-dom";

import { Icon } from "./Icon";
import { Dot, Emblem } from "./ui";
import { confirmAction } from "./Dialog";
import { api } from "../services/tauri";
import { useNovaStore } from "../stores/useNova";
import { toastInfo, toastSuccess, toastWarning } from "../stores/useToasts";
import { hideWindow } from "../lib/window";

/**
 * Quick-status popup mirroring the designed system-tray menu (Screen 7).
 * The real Windows tray menu lives in the backend (runtime.rs) with the same
 * entries; this panel is the in-app equivalent with full theming.
 */
export default function TrayPanel({
  onClose,
  statusText,
  dotKind,
}: {
  onClose: () => void;
  visual: string;
  statusText: string;
  dotKind: "ok" | "er" | "wn" | "mut";
}) {
  const navigate = useNavigate();
  const service = useNovaStore((store) => store.service);
  const microphone = useNovaStore((store) => store.microphone);

  const go = (route: string) => {
    onClose();
    navigate(route);
  };

  const toggleListening = async () => {
    onClose();
    if (service === "running") {
      await api.setServiceState("paused");
      toastWarning("Listening paused", "Wake-word detection is off — resume from here any time.");
    } else {
      await api.setServiceState("running");
      toastSuccess("Nova is listening", "Wake-word detection is on.");
    }
  };

  const exit = () => {
    onClose();
    confirmAction({
      title: "Exit Nova?",
      icon: "power",
      destructive: true,
      confirmLabel: "Exit Nova",
      body: (
        <p>
          Wake-word detection stops and the tray icon disappears. Your tasks, history and settings
          stay saved on this PC.
        </p>
      ),
      onConfirm: async () => {
        await api.quitNova();
      },
    });
  };

  return (
    <div className="trayHost">
      <div className="tray" role="menu" aria-label="Nova quick controls">
        <div className="trh">
          <Emblem size={30} />
          <div className="grow">
            <b>Nova</b>
            <div className="row" style={{ gap: 5 }}>
              <Dot kind={dotKind} live={service === "running" && microphone.available} />
              <span className="dim" style={{ fontSize: 11 }}>
                {statusText}
              </span>
            </div>
          </div>
        </div>

        <button className="ti" role="menuitem" onClick={() => go("/")}>
          <Icon name="exp" />
          <span className="grow" style={{ textAlign: "left" }}>
            Open Nova
          </span>
        </button>

        <button
          className={`ti${microphone.available ? "" : " dis"}`}
          role="menuitem"
          onClick={() => void toggleListening()}
        >
          <Icon name={service === "running" ? "pause" : "play"} />
          <span className="grow" style={{ textAlign: "left" }}>
            {service === "running" ? "Stop listening" : service === "stopped" ? "Start Nova" : "Start listening"}
          </span>
        </button>

        <button className="ti" role="menuitem" onClick={() => go("/tasks")}>
          <Icon name="tasks" />
          <span className="grow" style={{ textAlign: "left" }}>
            Open Tasks
          </span>
        </button>

        <button className="ti" role="menuitem" onClick={() => go("/activity")}>
          <Icon name="clock" />
          <span className="grow" style={{ textAlign: "left" }}>
            Recent activity
          </span>
        </button>

        <button className="ti" role="menuitem" onClick={() => go("/settings")}>
          <Icon name="set" />
          <span className="grow" style={{ textAlign: "left" }}>
            Settings
          </span>
        </button>

        {!microphone.available ? (
          <button
            className="ti"
            role="menuitem"
            onClick={() => {
              onClose();
              void api.openMicrophoneSettings();
              toastInfo("Opening Windows Settings", "Privacy & security → Microphone → allow Nova.");
            }}
          >
            <Icon name="micoff" />
            <span className="grow" style={{ textAlign: "left" }}>
              Open microphone settings
            </span>
          </button>
        ) : null}

        <div className="tsep" />

        <button className="ti dst" role="menuitem" onClick={exit}>
          <Icon name="power" />
          <span className="grow" style={{ textAlign: "left" }}>
            Exit Nova
          </span>
        </button>

        {service === "paused" ? (
          <div className="trnote">
            <Icon name="alert" size={13} />
            <span>Pausing isn’t quitting — Nova stays in the tray and keeps your tasks.</span>
          </div>
        ) : null}

        <button
          className="ti"
          role="menuitem"
          onClick={() => {
            onClose();
            void hideWindow();
          }}
        >
          <Icon name="min" />
          <span className="grow" style={{ textAlign: "left" }}>
            Hide window
          </span>
        </button>
      </div>
    </div>
  );
}
