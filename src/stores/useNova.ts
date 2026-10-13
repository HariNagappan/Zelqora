import { create } from "zustand";

import type {
  CommandPayload,
  ConfirmPayload,
  MicrophoneStatus,
  ResultPayload,
  ServiceState,
  StatePayload,
} from "../types";

/**
 * The overlay's visual state — the ten variants documented in the design.
 * Derived from the backend lifecycle state, never set directly.
 */
export type OverlayVisual =
  | "idle"
  | "wake"
  | "listening"
  | "transcribing"
  | "processing"
  | "confirm"
  | "executing"
  | "success"
  | "error"
  | "denied"
  | "cancelled"
  | "text";

interface NovaStore {
  state: string;
  detail: string | null;
  paused: boolean;
  service: ServiceState;
  microphone: MicrophoneStatus;
  overlayMode: "mini" | "expanded";
  transcript: string;
  command: CommandPayload | null;
  result: ResultPayload | null;
  confirm: ConfirmPayload;
  /** True while a confirmation/clarification answer is expected. */
  awaitingAnswer: boolean;
  /** True when the user opened the keyboard fallback in the overlay. */
  manualInput: boolean;
  /** Incremented whenever the activity log or task list changes. */
  activityBump: number;

  setState: (payload: StatePayload) => void;
  setTranscript: (text: string) => void;
  setCommand: (command: CommandPayload) => void;
  setResult: (result: ResultPayload) => void;
  setConfirm: (confirm: ConfirmPayload) => void;
  setMicrophone: (microphone: MicrophoneStatus) => void;
  setOverlayMode: (mode: "mini" | "expanded") => void;
  setManualInput: (open: boolean) => void;
  bumpActivity: () => void;
  /** Clear the per-interaction details when a new interaction starts. */
  beginInteraction: () => void;
}

const unknownMicrophone: MicrophoneStatus = {
  available: true,
  permission: "unknown",
  device: null,
  message: null,
};

export const useNovaStore = create<NovaStore>((set, get) => ({
  state: "IDLE",
  detail: null,
  paused: false,
  service: "running",
  microphone: unknownMicrophone,
  overlayMode: "mini",
  transcript: "",
  command: null,
  result: null,
  confirm: { pending: false, prompt: null, confirmLabel: null, kind: null, options: null },
  awaitingAnswer: false,
  manualInput: false,
  activityBump: 0,

  setState: (payload) => {
    const previous = get().state;
    const restarted = payload.state === "WAKE_DETECTED";
    set({
      state: payload.state,
      detail: payload.detail,
      paused: payload.paused,
      service: payload.service,
      ...(restarted
        ? { transcript: "", command: null, result: null, manualInput: false }
        : {}),
      ...(payload.state === "LISTENING_FOR_COMMAND" ? { manualInput: false } : {}),
    });
  },
  setTranscript: (text) => set({ transcript: text }),
  setCommand: (command) => set({ command }),
  setResult: (result) => set({ result, awaitingAnswer: false }),
  setConfirm: (confirm) =>
    set({
      confirm,
      awaitingAnswer: confirm.pending,
      ...(confirm.pending ? { manualInput: false } : {}),
    }),
  setMicrophone: (microphone) => set({ microphone }),
  setOverlayMode: (overlayMode) => set({ overlayMode }),
  setManualInput: (manualInput) => set({ manualInput }),
  bumpActivity: () => set((state) => ({ activityBump: state.activityBump + 1 })),

  beginInteraction: () =>
    set({ transcript: "", command: null, result: null, manualInput: false }),
}));

/**
 * Map the backend lifecycle state onto the overlay's visual variant.
 *
 * The rules are deliberately explicit: the overlay may only claim to be
 * listening once capture actually started, and may only claim success once the
 * backend reported a successful result.
 */
export function overlayVisual(
  state: string,
  service: ServiceState,
  microphone: MicrophoneStatus,
  manualInput: boolean,
  waitingForAnswer: boolean,
): OverlayVisual {
  if (manualInput) {
    return "text";
  }
  switch (state) {
    case "WAKE_DETECTED":
      return "wake";
    case "LISTENING_FOR_COMMAND":
      return "listening";
    case "TRANSCRIBING":
      return "transcribing";
    case "PROCESSING":
      return "processing";
    case "AWAITING_CONFIRMATION":
    case "AWAITING_CLARIFICATION":
      return waitingForAnswer ? "confirm" : "processing";
    case "EXECUTING":
      return "executing";
    case "SUCCESS":
    case "SPEAKING":
      return "success";
    case "ERROR":
      return "error";
    case "CANCELLED":
      return "cancelled";
    case "MICROPHONE_DENIED":
    case "MICROPHONE_UNAVAILABLE":
      return "denied";
    case "IDLE":
    case "LISTENING_FOR_WAKE_WORD":
    default:
      if (!microphone.available) {
        return "denied";
      }
      if (service === "stopped") {
        return "cancelled";
      }
      return "idle";
  }
}

/** Short status line shown under "Nova" in the overlay header. */
export function overlayStatusLabel(visual: OverlayVisual): string {
  switch (visual) {
    case "idle":
      return "Ready";
    case "wake":
      return "Wake word";
    case "listening":
      return "Listening…";
    case "transcribing":
      return "Transcribing…";
    case "processing":
      return "Thinking…";
    case "confirm":
      return "Confirm to continue";
    case "executing":
      return "Working…";
    case "success":
      return "Done";
    case "error":
      return "Something went wrong";
    case "denied":
      return "Microphone blocked";
    case "cancelled":
      return "Cancelled";
    case "text":
      return "Keyboard input";
    default:
      return "Ready";
  }
}
