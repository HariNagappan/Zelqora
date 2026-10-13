// Shared types mirroring the Rust backend payloads (snake_case fields come from
// serde defaults; camelCase is declared explicitly on the Rust side).
//
// Note: the provider credential is deliberately NOT part of `Settings`. It is
// write-only from the UI's point of view (see `api.setProviderCredential`), so
// the secret never travels back into the webview.

export type ThemeId =
  | "nova"
  | "darcula"
  | "midnight"
  | "oled"
  | "light"
  | "nord"
  | "dracula"
  | "soldark"
  | "sollight"
  | "system";

export type Density = "comfortable" | "compact" | "spacious";
export type Intensity = "minimal" | "balanced" | "immersive";
export type Verbosity = "brief" | "standard" | "detailed";
export type ServiceState = "running" | "paused" | "stopped";

export interface MicrophoneStatus {
  available: boolean;
  permission: "granted" | "denied" | "unknown";
  device: string | null;
  message: string | null;
}

export interface StatusInfo {
  state: string;
  detail: string | null;
  paused: boolean;
  service: ServiceState;
  wakeEnabled: boolean;
  wakeWord: string;
  modelReady: boolean;
  modelPath: string | null;
  ttsEnabled: boolean;
  sttProvider: "local" | "cloud";
  llmProvider: "auto" | "cloud" | "offline";
  llmConfigured: boolean;
  microphone: MicrophoneStatus;
  overlayMode: "mini" | "expanded";
  overlayEnabled: boolean;
  theme: ThemeId;
  onboardingComplete: boolean;
  version: string;
  credentialProtection: string;
}

/** Everything the Settings screen can read and write. */
export interface Settings {
  wake_word: string;
  wake_enabled: boolean;
  sensitivity: number;
  microphone: string | null;
  push_to_talk_hotkey: string;
  stt_provider: "local" | "cloud";
  tts_enabled: boolean;
  tts_voice: string | null;
  tts_rate: number;
  tts_volume: number;
  llm_provider: "auto" | "cloud" | "offline";
  openai_base_url: string;
  openai_model: string;
  confirm_destructive: boolean;
  autostart: boolean;
  start_minimized: boolean;
  overlay_enabled: boolean;
}

export interface BehaviourPrefs {
  response_verbosity: Verbosity;
  confirm_open: boolean;
  overlay_after_command: boolean;
  overlay_keep_expanded: boolean;
  history_retention_days: number;
  notifications_enabled: boolean;
}

export interface Effects {
  particles: boolean;
  network: boolean;
  parallax: boolean;
  voice: boolean;
  glow: number;
  transparency: number;
  intensity: Intensity;
}

export interface Appearance {
  theme: ThemeId;
  accent: string | null;
  density: Density;
  effects: Effects;
  customized: boolean;
}

export interface Onboarding {
  completed: boolean;
  completed_at: string | null;
  microphone_verified: boolean;
  first_command_done: boolean;
  providers_skipped: boolean;
}

export type Priority = "low" | "medium" | "high";
export type TaskView = "all" | "pending" | "completed" | "overdue";
export type TaskSort = "due" | "priority" | "created";

export interface Todo {
  id: number;
  title: string;
  description: string;
  completed: boolean;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
  priority: Priority;
  category: string;
  /** UTC RFC 3339, or null when the task has no due date. */
  due_at: string | null;
  /** voice | manual | onboarding */
  source: string;
}

export interface TaskCounts {
  all: number;
  pending: number;
  completed: number;
  overdue: number;
}

export interface TodayOverview {
  pendingToday: number;
  overdue: number;
  completedToday: number;
  openTotal: number;
}

export interface ActivityEntry {
  id: number;
  timestamp: string;
  transcript: string;
  intent: string | null;
  intent_label: string | null;
  parameters: string | null;
  result: string | null;
  success: boolean;
  response: string;
  duration_ms: number | null;
  status: string;
  heard: string;
  understood: string | null;
  action: string | null;
  retryable: boolean;
}

export interface VoiceInfo {
  id: string;
  name: string;
}

export interface DiagnosticCheck {
  name: string;
  ok: boolean;
  detail: string;
}

export interface Diagnostics {
  checks: DiagnosticCheck[];
  version: string;
  dataDir: string | null;
  databasePath: string | null;
  schemaVersion: number | null;
  modelPath: string | null;
  pushToTalk: string;
  credentialProtection: string;
}

export interface UpdateInfo {
  supported: boolean;
  available: boolean;
  message: string;
}

export interface TaskInput {
  title: string;
  description?: string | null;
  priority?: Priority;
  category?: string | null;
  /** ISO-8601 due date. */
  due?: string | null;
  source?: string;
}

export interface TaskPatch {
  title?: string;
  description?: string;
  priority?: Priority;
  category?: string;
  due?: string;
  clearDue?: boolean;
}

// ── event payloads ─────────────────────────────────────────────────────────

export interface StatePayload {
  state: string;
  detail: string | null;
  paused: boolean;
  service: ServiceState;
}

export interface TranscriptPayload {
  text: string;
  finalTranscript: boolean;
  source: string;
}

export interface CommandPayload {
  intent: string;
  label: string;
  summary: string;
}

export interface ResultPayload {
  intent: string | null;
  label: string | null;
  success: boolean;
  status: string;
  response: string;
  retryable: boolean;
  activityId: number | null;
}

export interface ConfirmPayload {
  pending: boolean;
  prompt: string | null;
  confirmLabel: string | null;
  kind: "confirmation" | "clarification" | null;
  /** Candidate answers offered for a clarification question. */
  options: string[] | null;
}

export interface NavigatePayload {
  page: string;
}

export interface LevelPayload {
  level: number;
  testing: boolean;
}

export interface ServicePayload {
  service: ServiceState;
  label: string;
  microphone: MicrophoneStatus;
}

export interface OverlayPayload {
  mode: "mini" | "expanded";
}
