// Typed wrappers around the backend `invoke` commands and event subscriptions.
//
// One subscription per window (guarded below) so no duplicate listeners can
// accumulate; `unsubscribeNovaEvents` removes all of them on teardown.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { setLevel } from "../lib/level";
import { useNovaStore } from "../stores/useNova";
import { useAppearanceStore } from "../stores/useAppearance";
import { notifyServiceChange } from "../stores/useService";
import type {
  ActivityEntry,
  Appearance,
  BehaviourPrefs,
  CommandPayload,
  ConfirmPayload,
  Diagnostics,
  LevelPayload,
  MicrophoneStatus,
  Onboarding,
  ResultPayload,
  ServicePayload,
  Settings,
  StatePayload,
  TaskCounts,
  TaskInput,
  TaskPatch,
  TaskSort,
  TaskView,
  TodayOverview,
  Todo,
  TranscriptPayload,
  UpdateInfo,
  VoiceInfo,
} from "../types";

export const api = {
  // status + settings
  getStatus: () => invoke<import("../types").StatusInfo>("get_status"),
  getSettings: () => invoke<Settings>("get_settings"),
  /** Partial update — only the fields present are changed. */
  updateSettings: (patch: Partial<Settings>) => invoke<Settings>("update_settings", { patch }),
  /** Write-only: the value is never read back into the UI. */
  setProviderCredential: (credential: string) =>
    invoke<Settings>("set_provider_credential", { credential }),
  clearProviderCredential: () => invoke<Settings>("clear_provider_credential"),

  // behaviour preferences
  getBehaviour: () => invoke<BehaviourPrefs>("get_behaviour"),
  updateBehaviour: (prefs: BehaviourPrefs) => invoke<BehaviourPrefs>("update_behaviour", { prefs }),

  // appearance
  getAppearance: () => invoke<Appearance>("get_appearance"),
  updateAppearance: (appearance: Appearance) =>
    invoke<Appearance>("update_appearance", { appearance }),
  resetAppearance: () => invoke<Appearance>("reset_appearance"),

  // onboarding
  getOnboarding: () => invoke<Onboarding>("get_onboarding"),
  updateOnboarding: (onboarding: Onboarding) =>
    invoke<Onboarding>("update_onboarding", { onboarding }),
  resetOnboarding: () => invoke<Onboarding>("reset_onboarding"),

  // audio + devices
  listMicrophones: () => invoke<string[]>("list_microphones"),
  listVoices: () => invoke<VoiceInfo[]>("list_voices"),
  getMicrophoneStatus: () => invoke<MicrophoneStatus>("get_microphone_status"),
  startMicrophoneTest: () => invoke<void>("start_microphone_test"),
  stopMicrophoneTest: () => invoke<void>("stop_microphone_test"),
  openMicrophoneSettings: () => invoke<void>("open_microphone_settings"),
  downloadModel: () => invoke<string>("download_model"),

  // tasks
  listTodos: (completed?: boolean | null) =>
    invoke<Todo[]>("list_todos", { completed: completed ?? null }),
  listTasks: (options: { view?: TaskView; query?: string; sort?: TaskSort } = {}) =>
    invoke<Todo[]>("list_tasks", {
      view: options.view ?? null,
      query: options.query ?? null,
      sort: options.sort ?? null,
    }),
  getTaskCounts: () => invoke<TaskCounts>("get_task_counts"),
  getTodo: (id: number) => invoke<Todo | null>("get_todo", { id }),
  createTask: (task: TaskInput) => invoke<Todo>("create_task", { task }),
  updateTask: (id: number, patch: TaskPatch) => invoke<Todo | null>("update_task", { id, patch }),
  setTodoCompleted: (id: number, completed: boolean) =>
    invoke<Todo | null>("set_todo_completed", { id, completed }),
  deleteTodo: (id: number) => invoke<boolean>("delete_todo", { id }),
  clearCompletedTodos: () => invoke<number>("clear_completed_todos"),
  searchTodos: (query: string) => invoke<Todo[]>("search_todos", { query }),
  getTodayOverview: () => invoke<TodayOverview>("get_today_overview"),

  // activity
  listActivity: (limit?: number, offset?: number) =>
    invoke<ActivityEntry[]>("list_activity", { limit: limit ?? null, offset: offset ?? null }),
  listActivityView: (options: {
    status?: "all" | "success" | "failed" | "cancelled";
    query?: string;
    limit?: number;
    offset?: number;
  }) =>
    invoke<ActivityEntry[]>("list_activity_view", {
      status: options.status && options.status !== "all" ? options.status : null,
      query: options.query ?? null,
      limit: options.limit ?? null,
      offset: options.offset ?? null,
    }),
  getActivityCounts: () => invoke<[number, number, number]>("get_activity_counts"),
  clearActivityHistory: () => invoke<number>("clear_activity_history"),
  retryActivity: (id: number) => invoke<string>("retry_activity", { id }),

  // runtime + desktop
  activate: () => invoke<void>("activate"),
  setPaused: (paused: boolean) => invoke<void>("set_paused", { paused }),
  setServiceState: (service: "running" | "paused" | "stopped") =>
    invoke<void>("set_service_state", { service }),
  confirmPending: (confirmed: boolean) => invoke<void>("confirm_pending", { confirmed }),
  processText: (text: string) => invoke<void>("process_text", { text }),
  cancelCurrent: () => invoke<void>("cancel_current"),
  showFloatingAssistant: () => invoke<void>("show_floating_assistant"),
  hideFloatingAssistant: () => invoke<void>("hide_floating_assistant"),
  setOverlaySize: (mode: "mini" | "expanded") => invoke<void>("set_overlay_size", { mode }),
  openMain: (page?: string) => invoke<void>("open_main", { page: page ?? null }),
  /** Quit Nova completely (releases the microphone and the global hotkey). */
  quitNova: () => invoke<void>("quit_nova"),

  // diagnostics
  getDiagnostics: () => invoke<Diagnostics>("get_diagnostics"),
  checkForUpdates: () => invoke<UpdateInfo>("check_for_updates"),
  openLogsFolder: () => invoke<string>("open_logs_folder"),
  openDocumentation: () => invoke<string>("open_documentation"),
};

let pending: Promise<UnlistenFn[]> | null = null;

/** Subscribe this window to backend events. Idempotent per window. */
export function subscribeNovaEvents(): Promise<UnlistenFn[]> {
  if (pending) {
    return pending;
  }
  const store = () => useNovaStore.getState();

  pending = Promise.all([
    listen<StatePayload>("nova:state", (event) => {
      store().setState(event.payload);
    }),
    listen<TranscriptPayload>("nova:transcript", (event) => {
      store().setTranscript(event.payload.text);
    }),
    listen<CommandPayload>("nova:command", (event) => {
      store().setCommand(event.payload);
    }),
    listen<ResultPayload>("nova:result", (event) => {
      store().setResult(event.payload);
    }),
    listen<ConfirmPayload>("nova:confirm", (event) => {
      store().setConfirm(event.payload);
    }),
    listen<LevelPayload>("nova:level", (event) => {
      // Levels bypass React entirely: the waveform reads this store directly.
      setLevel(event.payload.level, event.payload.testing);
    }),
    listen<MicrophoneStatus>("nova:microphone", (event) => {
      store().setMicrophone(event.payload);
    }),
    listen<ServicePayload>("nova:service", (event) => {
      notifyServiceChange(event.payload);
      store().setMicrophone(event.payload.microphone);
    }),
    listen<Appearance>("nova:appearance", (event) => {
      useAppearanceStore.getState().receiveApplied(event.payload);
    }),
    listen<{ mode: "mini" | "expanded" }>("nova:overlay", (event) => {
      store().setOverlayMode(event.payload.mode);
    }),
    listen("nova:activity", () => {
      store().bumpActivity();
    }),
  ]);

  return pending;
}

export function unsubscribeNovaEvents(): void {
  if (!pending) {
    return;
  }
  const current = pending;
  pending = null;
  current.then((unlisteners) => unlisteners.forEach((fn) => fn()));
}
