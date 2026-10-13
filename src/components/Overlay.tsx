// Floating voice assistant (design Screen 1).
//
// The window is frameless, always-on-top and transparent; the backend resizes
// it between the collapsed pill and the expanded panel. Every state below is
// driven by real backend state — the overlay never claims to be listening
// before capture started, and never reports success before the command
// actually succeeded.

import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { Icon } from "./Icon";
import { Emblem, Progress, Waveform } from "./ui";
import { api } from "../services/tauri";
import { useNovaStore, overlayStatusLabel, overlayVisual, type OverlayVisual } from "../stores/useNova";
import { toastInfo } from "../stores/useToasts";
import { isTauriRuntime } from "../lib/window";
import type { ResultPayload } from "../types";

/**
 * Design-review hatch: outside the Tauri shell the assistant can be rendered in
 * any state via `#/overlay?state=<id>`, which is how the overlay layouts are
 * compared against `design/原型/V3 Nova-Prototype.html`. It has no effect inside
 * the real application.
 */
function forcedPreviewState(): OverlayVisual | null {
  if (isTauriRuntime()) {
    return null;
  }
  const query = window.location.hash.split("?")[1];
  if (!query) {
    return null;
  }
  const value = new URLSearchParams(query).get("state");
  const known: OverlayVisual[] = [
    "idle",
    "wake",
    "listening",
    "transcribing",
    "processing",
    "confirm",
    "executing",
    "success",
    "error",
    "denied",
    "cancelled",
    "text",
    "mini" as OverlayVisual,
  ];
  return value && known.includes(value as OverlayVisual) ? (value as OverlayVisual) : null;
}

/**
 * Design-review hatch companion: `#/overlay?state=error&result=not_heard`
 * seeds a representative result so the error variants can be inspected.
 */
function forcedPreviewResult(): ResultPayload | null {
  if (isTauriRuntime()) {
    return null;
  }
  const query = window.location.hash.split("?")[1];
  if (!query) {
    return null;
  }
  const value = new URLSearchParams(query).get("result");
  const base = { intent: null, label: null, success: false, activityId: null } as const;
  switch (value) {
    case "not_heard":
      return {
        ...base,
        status: "not_heard",
        response: "I didn't catch that. Try again, or type the command instead.",
        retryable: true,
      };
    case "unsupported":
      return {
        ...base,
        status: "unsupported",
        response: "I don't know how to do that yet. I can manage your tasks and open apps, files and folders.",
        retryable: false,
      };
    case "failed":
      return {
        ...base,
        status: "failed",
        response: "Couldn't reach the speech provider (network timeout). Retry, or check provider settings.",
        retryable: true,
        activityId: 1,
      };
    default:
      return null;
  }
}

export default function Overlay() {
  const nova = useNovaStore();
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement | null>(null);

  const visual = forcedPreviewState()
    ? (forcedPreviewState() as OverlayVisual)
    : overlayVisual(
        nova.state,
        nova.service,
        nova.microphone,
        nova.manualInput,
        nova.awaitingAnswer,
      );
  const mini =
    visual === ("mini" as OverlayVisual)
      ? true
      : forcedPreviewState() !== null
        ? false
        : nova.overlayMode === "mini" && !nova.manualInput;

  useEffect(() => {
    document.body.classList.toggle("mini", mini);
  }, [mini]);

  // While an interaction is running the panel expands; when nothing is
  // happening it collapses back to the pill unless the user keeps it open.
  useEffect(() => {
    const active = ["wake", "listening", "transcribing", "processing", "confirm", "executing", "error", "denied"].includes(
      visual,
    );
    if (active && nova.overlayMode !== "expanded") {
      void api.setOverlaySize("expanded");
    }
  }, [nova.overlayMode, visual]);

  useEffect(() => {
    if (visual === "text" || visual === "listening") {
      inputRef.current?.focus();
    }
  }, [visual]);

  // Esc stops the current interaction (or dismisses the pill).
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      if (nova.manualInput) {
        nova.setManualInput(false);
        return;
      }
      void api.cancelCurrent();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [nova]);

  // The overlay window may load after the first `nova:overlay` event, so it
  // asks the backend for its current mode once (and mirrors later events).
  useEffect(() => {
    void api
      .getStatus()
      .then((status) => nova.setOverlayMode(status.overlayMode))
      .catch(() => undefined);
    const previewResult = forcedPreviewResult();
    if (previewResult) {
      nova.setResult(previewResult);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The overlay mirrors appearance changes coming from the settings window.
  useEffect(() => {
    const pending = listen("nova:overlay", () => {
      /* mode changes are mirrored into the store by services/tauri.ts */
    });
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, []);

  const send = async () => {
    const text = draft.trim();
    if (!text) {
      return;
    }
    setDraft("");
    try {
      await api.processText(text);
    } catch (error) {
      toastInfo("Nova couldn't run that", String(error));
    }
  };

  const micButton = () => {
    if (visual === "listening") {
      void api.cancelCurrent();
      return;
    }
    void api.activate();
  };

  const emblemState = (): "" | "lis" | "pro" | "ok" | "er" => {
    switch (visual) {
      case "listening":
      case "wake":
        return "lis";
      case "processing":
      case "transcribing":
      case "executing":
      case "confirm":
        return "pro";
      case "success":
        return "ok";
      case "error":
        return "er";
      default:
        return "";
    }
  };

  if (mini) {
    const live = nova.service === "running" && nova.microphone.available;
    return (
      <div
        className="vo mini"
        role="region"
        aria-label="Nova voice assistant (collapsed)"
        data-s="mini"
        title="Open the Nova assistant"
        onClick={() => void api.setOverlaySize("expanded")}
      >
        <div className="vh" style={{ padding: "7px 12px 7px 9px" }}>
          <Emblem size={28} />
          <div className="vn" style={{ marginRight: 2 }}>
            Nova
          </div>
          <span className={`dot ${live ? "ok live" : "mut"}`} aria-hidden="true" />
          <span className="dim" style={{ fontSize: 11 }}>
            {nova.microphone.available ? (live ? "Hey Nova" : "Paused") : "Mic off"}
          </span>
        </div>
      </div>
    );
  }

  return (
    <div className="vo" role="region" aria-label="Nova voice assistant" data-s={visual}>
      <div className="vh">
        <Emblem size={28} state={emblemState()} />
        <div className="grow">
          <div className="vn">Nova</div>
          <div className="vst">{overlayStatusLabel(visual)}</div>
        </div>
        <button
          className="ibt"
          aria-label="Collapse the assistant"
          title="Collapse the assistant"
          onClick={() => {
            void api.setOverlaySize("mini");
            if (nova.service === "running") {
              toastInfo("Nova keeps listening", "The wake word still works while collapsed.");
            }
          }}
        >
          <Icon name="x" />
        </button>
      </div>

      <div className="vb" aria-live="polite">
        <Body
          visual={visual}
          draft={draft}
          inputRef={inputRef}
          onDraft={setDraft}
          onSend={send}
        />
      </div>

      <div className="vf">
        <button
          className={`mbtn${visual === "listening" ? "" : " off"}`}
          aria-label={visual === "listening" ? "Stop listening" : "Start listening"}
          title={visual === "listening" ? "Stop listening" : "Start listening"}
          onClick={micButton}
        >
          <Icon name={visual === "listening" ? "mic" : "micoff"} size={18} />
        </button>
        <button
          className="ibt"
          aria-label="Type a command instead"
          title="Type a command instead"
          onClick={() => nova.setManualInput(true)}
        >
          <Icon name="kbd" />
        </button>
        <button
          className="ibt"
          aria-label="Open voice settings"
          title="Open voice settings"
          onClick={() => void api.openMain("settings")}
        >
          <Icon name="set" />
        </button>
        <span className="grow" />
        <span className="dim" style={{ fontSize: 10.5 }}>
          {nova.service === "running" ? "Wake word on" : nova.service === "paused" ? "Paused" : "Stopped"}
        </span>
      </div>
    </div>
  );
}

function Body({
  visual,
  draft,
  inputRef,
  onDraft,
  onSend,
}: {
  visual: string;
  draft: string;
  inputRef: React.MutableRefObject<HTMLInputElement | null>;
  onDraft: (value: string) => void;
  onSend: () => void;
}) {
  const nova = useNovaStore();
  const transcript = nova.transcript;
  const command = nova.command;
  const result = nova.result;
  const confirm = nova.confirm;

  switch (visual) {
    case "wake":
      return (
        <>
          <Waveform running={false} bars={[6, 13, 20, 13, 6]} />
          <div className="vt">Wake word detected</div>
          <div className="vs">Go ahead — Nova is listening.</div>
        </>
      );

    case "listening":
      return (
        <>
          <span className="vring" aria-hidden="true" />
          <Waveform running />
          <div className="qbox">{transcript ? `${transcript}…` : "…"}</div>
        </>
      );

    case "transcribing":
      return (
        <>
          <div className="row">
            <span className="spn" />
            <div className="vt">Transcribing…</div>
          </div>
          <div className="qbox">{transcript || "…"}</div>
        </>
      );

    case "processing":
      return (
        <>
          <div className="row">
            <span className="spn" />
            <div className="vt">Thinking…</div>
          </div>
          <div className="qbox">{transcript ? `“${transcript}”` : command?.summary ?? ""}</div>
        </>
      );

    case "confirm":
      return (
        <>
          <div className="conf">
            <div className="ct">{confirm.kind === "clarification" ? "Which one?" : "Nova will"}</div>
            <div className="vt" style={{ fontSize: 13.5 }}>
              {confirm.prompt ?? ""}
            </div>
          </div>
          {confirm.kind === "clarification" && confirm.options?.length ? (
            <div className="row" style={{ gap: 6, flexWrap: "wrap" }}>
              {confirm.options.map((option) => (
                <button
                  key={option}
                  className="btn sec sm"
                  onClick={() => void api.processText(option.replace(/^'|'$/g, ""))}
                >
                  {option}
                </button>
              ))}
            </div>
          ) : (
            <div className="row" style={{ gap: 8 }}>
              <button className="btn pri sm" onClick={() => void api.confirmPending(true)}>
                {confirm.confirmLabel ?? "Confirm"}
              </button>
              <button className="btn sec sm" onClick={() => void api.confirmPending(false)}>
                Cancel
              </button>
            </div>
          )}
          {confirm.kind === "clarification" ? (
            <div className="row" style={{ gap: 8 }}>
              <button className="btn gst sm" onClick={() => void api.cancelCurrent()}>
                Never mind
              </button>
              <span className="dim" style={{ fontSize: 11.5 }}>
                Or say the task name.
              </span>
            </div>
          ) : null}
        </>
      );

    case "executing":
      return (
        <>
          <div className="vt">{command?.summary ?? "Working on it…"}</div>
          <Progress />
          <div className="vs">Running the action now.</div>
        </>
      );

    case "success":
      return (
        <>
          <div className="row">
            <span className="cic ok">
              <Icon name="check" size={20} />
            </span>
            <div>
              <div className="vt">{result?.label ?? "Done"}</div>
              {(result?.response ?? "").trim() ? (
                <div className="vs">{result?.response}</div>
              ) : null}
            </div>
          </div>
        </>
      );

    case "error":
      // Three distinct designed outcomes share the error state: nothing was
      // heard, the request is not supported, or the command genuinely failed.
      // They get different recovery actions because they need different fixes.
      if (result?.status === "not_heard") {
        return (
          <>
            <div className="row">
              <span className="cic wn">
                <Icon name="mic" size={18} />
              </span>
              <div className="grow">
                <div className="vt">Couldn’t hear you clearly</div>
                <div className="vs">Nothing was changed. Try again, or type it instead.</div>
              </div>
            </div>
            <div className="row" style={{ gap: 8 }}>
              <button className="btn pri sm" onClick={() => void api.activate()}>
                <Icon name="mic" />
                Try again
              </button>
              <button className="btn sec sm" onClick={() => nova.setManualInput(true)}>
                <Icon name="kbd" />
                Type instead
              </button>
            </div>
          </>
        );
      }
      if (result?.status === "unsupported") {
        return (
          <>
            <div className="row">
              <span className="cic wn">
                <Icon name="info" size={18} />
              </span>
              <div className="grow">
                <div className="vt">That isn’t supported yet</div>
                <div className="vs">
                  {result?.response ??
                    "Nova can manage your tasks and open apps, files and folders."}
                </div>
              </div>
            </div>
            <div className="row" style={{ gap: 8 }}>
              <button
                className="btn sec sm"
                onClick={async () => {
                  try {
                    toastInfo("Opening documentation…", await api.openDocumentation());
                  } catch (error) {
                    toastInfo("Documentation isn’t available", String(error));
                  }
                }}
              >
                See what Nova can do
              </button>
              <button className="btn gst sm" onClick={() => void api.cancelCurrent()}>
                Dismiss
              </button>
            </div>
          </>
        );
      }
      return (
        <>
          <div className="row">
            <span className="cic er">
              <Icon name="alert" size={18} />
            </span>
            <div className="grow">
              <div className="vt">Couldn’t finish that</div>
              <div className="vs">
                {(result?.response ?? nova.detail ?? "").trim() ||
                  "No further detail is available — Settings → About → Run diagnostics can help."}
              </div>
            </div>
          </div>
          <div className="row" style={{ gap: 8 }}>
            <button
              className="btn sec sm"
              disabled={!result?.retryable || result.activityId == null}
              title={
                result?.retryable
                  ? "Run the command again"
                  : "This command can't be retried — it needs new input"
              }
              onClick={() => {
                if (result?.activityId != null) {
                  void api.retryActivity(result.activityId);
                }
              }}
            >
              <Icon name="refresh" />
              Retry
            </button>
            <button className="btn gst sm" onClick={() => void api.openMain("settings")}>
              Open settings
            </button>
          </div>
        </>
      );

    case "denied":
      return (
        <>
          <div className="row">
            <span className="cic wn">
              <Icon name="micoff" size={18} />
            </span>
            <div className="grow">
              <div className="vt">
                {nova.microphone.permission === "denied"
                  ? "Microphone access is off"
                  : "No microphone available"}
              </div>
              <div className="vs">
                {nova.microphone.permission === "denied"
                  ? "Allow Nova in Windows Settings → Privacy & security → Microphone."
                  : nova.microphone.message ?? "Plug one in, or check Windows sound settings."}
              </div>
            </div>
          </div>
          <div className="row" style={{ gap: 8 }}>
            <button className="btn sec sm" onClick={() => void api.openMicrophoneSettings()}>
              Open Windows Settings
            </button>
            <button className="btn gst sm" onClick={() => void api.hideFloatingAssistant()}>
              Dismiss
            </button>
          </div>
        </>
      );

    case "cancelled":
      return (
        <>
          <div className="row">
            <span className="cic wn">
              <Icon name="x" size={16} />
            </span>
            <div>
              <div className="vt">
                {nova.service === "stopped"
                  ? "Nova is stopped"
                  : nova.service === "paused"
                    ? "Listening paused"
                    : "Cancelled — nothing changed"}
              </div>
              <div className="vs">
                {nova.service === "running"
                  ? nova.detail ?? "Nothing was changed."
                  : "Start it again from here or from the tray."}
              </div>
            </div>
          </div>
          {nova.service !== "running" ? (
            <div className="row" style={{ gap: 8 }}>
              <button className="btn pri sm" onClick={() => void api.setServiceState("running")}>
                Start Nova
              </button>
            </div>
          ) : null}
        </>
      );

    case "text":
      return (
        <>
          <div className="vt">Type a command instead</div>
          <div className="txtin">
            <input
              className="inp"
              ref={inputRef}
              value={draft}
              placeholder="e.g. add buy milk tomorrow"
              aria-label="Type a command"
              onChange={(event) => onDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  onSend();
                }
              }}
            />
            <button className="btn pri sm" onClick={onSend} disabled={!draft.trim()}>
              Send
            </button>
          </div>
          <div className="hint">
            Runs through exactly the same command pipeline as your voice.
          </div>
        </>
      );

    case "idle":
    default:
      return (
        <>
          <div className="vt">
            {nova.service === "running"
              ? "I’m listening for the wake word."
              : "Nova is waiting — start it to use the wake word."}
          </div>
          <div className="vs">
            Add tasks, check your day, or open apps, files and folders — or type below.
          </div>
          <div className="row" style={{ gap: 8 }}>
            <button className="btn sec sm" onClick={() => nova.setManualInput(true)}>
              <Icon name="kbd" />
              Type instead
            </button>
            <button className="btn gst sm" onClick={() => void api.openMain("home")}>
              Open Nova
            </button>
          </div>
        </>
      );
  }
}
