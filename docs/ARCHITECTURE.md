# Nova — architecture

This document describes the implemented system: what owns what, how the
windows talk to each other, and where each piece of state lives. It reflects
the code in `src/` and `src-tauri/`, not an aspiration.

## Layers

| Layer | Technology | Location |
| --- | --- | --- |
| Dashboard UI | React 18 + TypeScript + Vite | `src/` |
| Floating assistant | Same bundle, `overlay` window | `src/components/Overlay.tsx` |
| Desktop shell | Tauri 2 (windows, tray, IPC, autostart, global shortcut) | `src-tauri/src/runtime.rs`, `lib.rs` |
| Agent core | Rust: orchestrator, context, intent providers | `src-tauri/src/agent/` |
| Command layer | Rust: registry, validator, executor | `src-tauri/src/commands/` |
| Skills | Rust: tasks, Windows automation | `src-tauri/src/skills/` |
| Audio | Rust: cpal capture, VAD, resampler, level meter | `src-tauri/src/audio/` |
| Speech | Rust + whisper.cpp worker, optional cloud STT | `src-tauri/src/stt/` |
| Speech output | Rust (`tts` crate → Windows voices) | `src-tauri/src/tts/` |
| Appearance | Rust model + CSS token tables | `src-tauri/src/appearance.rs`, `src/theme/themes.ts` |
| Persistence | SQLite (rusqlite, bundled) + embedded migrations | `src-tauri/src/database/`, `migrations/` |
| Secrets | Windows DPAPI (current user) | `src-tauri/src/secrets.rs` |

## Separation of concerns

Nova keeps six concerns apart, and the boundaries are enforced in code rather
than by convention:

1. **UI presentation** — `src/components`, `src/pages`. Consumes theme
   variables and store selectors; never talks to SQLite directly.
2. **Application state** — zustand stores in `src/stores`. Each store owns one
   subject: voice lifecycle, tasks, appearance, preferences, toasts, status.
3. **Voice interaction & orchestration** — `audio/`, `agent/`, `runtime.rs`.
   The state machine and the event loop live here.
4. **Task management** — `skills::todo` (voice path) and the `list_tasks` /
   `create_task` / `update_task` commands (UI path) share the same database
   functions, so both paths see identical data.
5. **Windows/desktop integration** — `windows/` (ShellExecute, known folders,
   Start-Menu index) and `runtime.rs` (tray, overlay sizing, autostart,
   global shortcut).
6. **Persistence** — `database/mod.rs` is the only module that writes SQL.

There is deliberately **no second source of truth**: the frontend never caches
server state across sessions, and every mutation goes through a command.

## State ownership

| State | Owner | Observed by |
| --- | --- | --- |
| Assistant lifecycle (`IDLE` … `ERROR`) | `AppState.ctl.state` (Rust) | `nova:state` event → `useNova` store |
| Speech-recognition progress | `audio` pipeline → `TranscribeStart`/`TranscriptReady` | `nova:state`, `nova:transcript` |
| Interpreted command | `agent` → `emit_command` | `nova:command` → overlay |
| Result + activity id | `agent::finalize_turn` | `nova:result`, `nova:activity` |
| Confirmation / clarification | `AgentSession.pending` / `.clarification` | `nova:confirm` |
| Service state (running/paused/stopped) | `AppState.ctl.service` | `nova:state`, `nova:service` |
| Microphone availability | `AppState.ctl.microphone` | `nova:microphone` |
| Input level (0..1) | audio thread | `nova:level` → `src/lib/level.ts` (outside React) |
| Tasks | SQLite | `useTasks` store (reloaded on `nova:activity`) |
| Applied appearance | SQLite row `appearance` | `useAppearance` store |
| Theme preview | `useAppearance.preview` (frontend only) | Appearance page scope |
| Behaviour preferences | SQLite row `behaviour` | Settings + `runtime::apply_behaviour_change` |
| Onboarding | SQLite row `onboarding` | `useOnboarding` store |

### Why the level meter bypasses React

Levels arrive ~20 times per second. `src/lib/level.ts` is a tiny external store;
`Waveform` and `LevelMeter` subscribe and write `style.height` on their own DOM
nodes inside a `requestAnimationFrame` loop. No component re-renders for audio,
and the ambient canvas reads the same object instead of React state.

## Event contracts (Rust → UI)

All payloads are typed on both sides (`src-tauri/src/events.rs` ↔ `src/types.ts`).

| Event | Payload | Emitted when |
| --- | --- | --- |
| `nova:state` | `{state, detail, paused, service}` | every accepted state transition |
| `nova:transcript` | `{text, finalTranscript, source}` | a voice transcript is ready |
| `nova:command` | `{intent, label, summary}` | an intent passed validation |
| `nova:result` | `{intent, label, success, status, response, retryable, activityId}` | a turn finished (not for prompts) |
| `nova:confirm` | `{pending, prompt, confirmLabel, kind, options}` | confirmation or clarification needed |
| `nova:activity` | `{}` | the activity log changed (tasks/UI refresh trigger) |
| `nova:level` | `{level, testing}` | while capturing or during the mic test |
| `nova:microphone` | `MicrophoneStatus` | device/permission changes |
| `nova:service` | `{service, label, microphone}` | assistant started/paused/stopped |
| `nova:appearance` | `Appearance` | the applied appearance changed (mirrored to the overlay) |
| `nova:overlay` | `{mode}` | the overlay window was resized (mini/expanded) |
| `nova:navigate` | `{page}` | tray or single-instance navigation request |

UI → Rust uses `invoke` commands only (see `src/services/tauri.ts`). One
subscription per window is created in `subscribeNovaEvents()`; every listener is
removed on unmount, so hot-reload and window reloads cannot accumulate
duplicates.

## Voice lifecycle and its guard rails

```
idle → listening_for_wake_word → wake_detected → listening_for_command
     → transcribing → processing → (awaiting_clarification) → (awaiting_confirmation)
     → executing → success → speaking → idle
```

- `events::nova_can_transition` encodes the legal transitions. `runtime::set_state`
  refuses anything else, logs it, and keeps the previous state.
- Every state may recover to `IDLE`, `LISTENING_FOR_WAKE_WORD`, `CANCELLED`,
  `ERROR`, `MICROPHONE_DENIED` or `MICROPHONE_UNAVAILABLE`, so a rejected
  transition can never wedge the assistant.
- The overlay derives its visual state from the backend state
  (`overlayVisual()`), which is why it never claims to be listening before
  capture started, and never shows “Done” before the skill succeeded.
- Confirmations expire after 60 s, clarification questions after 45 s; a
  capture that hears nothing times out in 6 s. Each timeout lands in a terminal
  state that schedules the return to rest.

## Command safety

1. An utterance is interpreted into `CommandRequest { intent, parameters }`
   (offline rules, or a cloud model restricted to the tool schema).
2. `validator::validate` rejects unknown intents, unknown parameters, missing
   required values and malformed values — and normalizes priorities, scopes and
   ISO due dates. Nothing executes before this passes.
3. `registry` supplies the risk level: `SAFE`, `CONFIRMATION_REQUIRED` or
   `BLOCKED`. Blocked intents are refused even if a model emits them.
4. The confirmation gate handles `DELETE_TODO`, `CLEAR_COMPLETED_TODOS` and
   (optionally) the `OPEN_*` intents. Ambiguous task references produce a
   clarification question instead of a guess.
5. `executor::execute` dispatches to a skill; there is no shell, no script
   runner, and no path from transcript text to a process launch.

### The one external process

`SHUTDOWN_PC` is the only intent that starts a process, and it is deliberately
narrow:

- the argument vector is fixed in code (`shutdown /s /t <clamped seconds>`, and
  `shutdown /a` for `CANCEL_SHUTDOWN`) — no transcript, model output or setting
  is interpolated into it;
- the risk level is `CONFIRMATION_REQUIRED`, and the confirmation is a separate
  turn the user must approve by voice or button;
- the countdown gives the user a real way back (`cancel the shutdown`);
- a parsing guard keeps the intent unreachable for anything but an explicit
  request that names the machine (`shut down my PC`, `turn off the laptop`), and
  never for Nova itself.

Everything else that touches the file system or launches software goes through
`ShellExecuteW` with the “open” verb, gated by the executable/script extension
guard.

## Persistence and migrations

- Migrations live in `migrations/` and are embedded at build time; the runner
  applies everything newer than `PRAGMA user_version`.
- `0001_init.sql` created tasks, activity and settings.
- `0002_tasks_appearance.sql` adds task `priority`, `category`, `due_at`,
  `source` and the richer activity columns (`status`, `heard`, `understood`,
  `action`, `retryable`, `intent_label`) plus indexes. It is additive: existing
  rows keep their data and receive defaults (covered by a test).
- Due dates are stored as **UTC** RFC 3339 so ordering and the overdue filter
  are correct regardless of the local offset; the UI renders them in local time.
- Settings, behaviour, appearance and onboarding are single JSON rows in the
  `settings` table, loaded with `load_json_or_default` so a corrupt row can
  never brick startup.
- The provider credential is DPAPI-encrypted before it is written and decrypted
  on load (`database::encode_settings` / `decode_settings`).

## Desktop behaviour

- Two windows: `main` (decorated, 1180×780, custom title bar) and `overlay`
  (frameless, transparent, always-on-top, click-through-free, resized between
  the 268×60 pill and the 400×268 panel by `runtime::set_overlay_mode`).
- Closing the main window hides it; `RunEvent::ExitRequested` is prevented so
  the tray keeps running. Exit is explicit (tray “Exit Nova”, the in-app tray
  panel, or `quit_nova`), and it releases the microphone, hotkey and audio.
- The tray menu mirrors the design and its labels follow the service state;
  the tooltip and status line follow the lifecycle state.
- Autostart is synchronised with the `autostart` preference; the global
  push-to-talk shortcut is unregistered and re-registered on change.
- Single instance is enforced; a second launch focuses the dashboard.

## Failure handling

| Failure | Behaviour |
| --- | --- |
| No microphone / device error | `MICROPHONE_UNAVAILABLE`, message from the driver, overlay offers Windows sound settings |
| Permission denied | `MICROPHONE_DENIED`, overlay offers Windows Privacy settings, wake listening stays off |
| Speech model missing | wake word disabled, dashboard says so, download button in Settings |
| Cloud provider unreachable | offline fallback is used and marked in the activity log; hard failures are `retryable` |
| Database error | command reports the failure, activity keeps the previous entries, UI shows a retry |
| Ambiguous task | clarification question with candidate buttons; the next utterance resolves it |
| Duplicate submission | an identical transcript within 2.5 s is logged once and not re-executed |

## Testing map

| Area | Where |
| --- | --- |
| Offline parser, slots, edit/summary intents | `agent/llm/offline.rs`, `agent/llm/slots.rs` |
| Task resolution, due descriptions, summaries | `skills/todo.rs` |
| Validation, risk levels, normalization | `commands/validator.rs` |
| Registry consistency, tool schema, labels | `commands/registry.rs` |
| State machine + recovery rules | `events.rs` |
| Appearance catalogue, normalization, contrast | `appearance.rs` |
| Behaviour clamping, verbosity rendering | `preferences.rs` |
| Credential protection | `secrets.rs` |
| Migrations, CRUD, filters, retention | `database/mod.rs` |
| End-to-end agent flow (mock runtime) | `src-tauri/tests/agent_pipeline.rs` |
| Speech E2E (ignored by default) | `src-tauri/tests/speech_e2e.rs` |
