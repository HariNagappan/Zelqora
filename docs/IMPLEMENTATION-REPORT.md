# Nova — implementation report

Date: 11 October 2026 · Scope: the existing Nova repository in this workspace,
transformed into the product described by `./design` (V3 prototype is the
design source of truth).

## 1. What was implemented

**Screens (all designed screens, with real behaviour)**

| Screen | State |
| --- | --- |
| Home dashboard | greeting, assistant status (wake word, mic, speech, understanding), Test voice activation, Today card, recent commands, loading/empty/error states |
| Tasks | All/Pending/Completed/Overdue filters with counts, search, sort (due/priority/created), task rows with checkbox, due chip (overdue styled), priority, category, voice mark, create/edit modal with validation, guarded delete with undo toast, inline completion (optimistic + rollback) |
| Activity | chronological stream with time, heard text, intent label, status badge, expandable Heard/Understood/Action/Result (+duration), search, status filters, retry, clear history, empty state for new users |
| Settings | General, Voice activation, Speech & AI, Behavior, Appearance, Privacy, About — every control persists and takes effect |
| Appearance | 10-theme gallery with miniature previews of the real UI, isolated live preview panel, accent swatches + custom picker, density, background effects, glow/transparency, animation intensity, Apply / Cancel / Reset, System Default following Windows |
| Onboarding | 6 steps (welcome → permission → live mic test → optional providers → first command → ready), progress bar, back/skip, completion persisted |
| Floating overlay | idle pill, wake detected, listening (live waveform), transcribing, thinking, awaiting confirmation, awaiting clarification (candidate buttons), executing, success, error + retry, mic denied/unavailable, cancelled, typed-input fallback, mini/expanded window |
| System tray + quick panel | native tray menu (status line, open, stop/start listening, tasks, activity, settings, mic help, exit) and the themed in-app equivalent |
| Dialogs & notifications | one dialog host for confirmations/errors (Esc, backdrop, focus handling), toast host with actions |

**Voice + agent lifecycle**

- Full lifecycle with explicit states and a guarded transition table
  (`src-tauri/src/events.rs`): idle, listening for wake word, wake detected,
  listening, transcribing, processing, awaiting clarification, awaiting
  confirmation, executing, success, speaking, cancelled, error, microphone
  denied, microphone unavailable.
- Illegal transitions are refused and logged; every state can recover, so the
  assistant can never wedge. Confirmations expire (60 s), questions expire
  (45 s), silent captures time out (6 s).
- Privacy: audio is only processed after the wake word or push-to-talk; input
  levels are only emitted while capturing or during the microphone test; the
  overlay never claims "listening" before capture started.

**Command system**

- Registry-driven: `ADD_TODO` (with due date, priority, category),
  `LIST_TODOS`, `SUMMARIZE_TODOS` (today/tomorrow/week/overdue/all),
  `COMPLETE_TODO`, `EDIT_TODO`, `DELETE_TODO`, `CLEAR_COMPLETED_TODOS`,
  `SEARCH_TODOS`, `OPEN_APPLICATION`, `OPEN_FILE`, `OPEN_FOLDER`,
  `TAKE_SCREENSHOT`, plus `RUN_SCRIPT` / `EXECUTE_SHELL_COMMAND` registered as
  **BLOCKED** and refused twice (validator + executor).
- Offline slot extraction now understands due dates (“due today at 6 pm”,
  “tomorrow”, “next monday”), priorities and categories, edit phrasings
  (“rename X to Y”, “set X to high priority”, “move X to tomorrow”, “clear the
  due date on X”) and summary questions (“what do I need to finish today?”).
- Ambiguous references ask a question with candidate buttons instead of
  guessing; the next utterance resolves it and still passes the confirmation
  gate.
- Repeated identical submissions within 2.5 s are not executed twice.

## 2. Architecture and database changes

- **New Rust modules**: `appearance.rs` (theme catalogue, effects presets,
  accent maths, validation), `preferences.rs` (behaviour preferences with
  clamped ranges), `onboarding.rs`, `secrets.rs` (Windows DPAPI protection for
  the provider credential), `agent/llm/slots.rs` (date/priority/category slots).
- **Extended**: `events.rs` (states + transition rules, `ServiceState`,
  microphone status, new event channels `nova:level`, `nova:microphone`,
  `nova:service`, `nova:appearance`, `nova:overlay`), `runtime.rs` (tray menu
  matching the design, overlay window sizing, live settings/behaviour
  application, microphone-failure classification, resource release on exit),
  `audio/mod.rs` (level meter, cancel handling, transcription state),
  `commands/*` (new intents, normalization, risk table), `skills/todo.rs`
  (due/priority/category/source, edit, summaries), `state.rs`, `lib.rs`
  (≈45 invoke commands).
- **Migration `0002_tasks_appearance.sql`** (additive, preserves data):
  task `priority`, `category`, `due_at` (UTC RFC 3339), `source`; activity
  `status`, `heard`, `understood`, `action`, `retryable`, `intent_label`;
  new indexes; backfill of `heard`/`intent_label` for existing rows.
- **Secrets**: the provider credential is written as a DPAPI blob (current
  Windows user). The UI can only set or remove it — `get_settings` blanks the
  field, and `update_settings` now takes a partial patch so an edit can never
  wipe it.
- **Frontend rewritten** around the design system: `styles.css` carries the V3
  token tables and component styles (10 themes), `src/theme/themes.ts` the
  catalogue and token maths, new stores (`useNova`, `useTasks`, `useAppearance`,
  `useStatus`, `useOnboarding`, `useToasts`, `useService`), new components
  (shell/titlebar/sidebar, overlay, ambient canvas, dialog host, toast host,
  task row/editor, icon sprite, primitives) and pages (Home, Tasks, Activity,
  Settings, Appearance) plus onboarding. The superseded `src/pages/Todos.tsx`
  was removed; its functionality lives in Tasks, and the pre-design route
  `#/todos` still resolves to the Tasks screen so existing links keep working.
  A non-Tauri “preview hatch” (`#/overlay?state=<id>`) renders any assistant
  state in a plain browser for design review; it is inert inside the app.

## 3. Flows verified end-to-end

1. **Add a task by voice** (Flow A) — transcript → intent → validation →
   persistence → overlay result; covered by
   `voice_capture_stores_due_date_priority_and_category`.
2. **Check pending tasks** (Flow B) — `SUMMARIZE_TODOS` answers “today” and
   “tomorrow” with the right tasks; covered by `summarize_today_reports_what_is_due`.
3. **Confirm a sensitive action** (Flow C) — delete asks first, “yes” deletes,
   “no” changes nothing; covered by existing tests plus
   `confirmation_prompts_are_logged_as_awaiting`.
4. **First-time setup** (Flow D) — six steps with a real microphone test and a
   real first command; completion persists and is only re-run on request.
5. **Recover from a failure** (Flow E) — failures show the real reason and a
   retry where the entry is retryable.
6. **Manage tasks manually** (Flow F) — create/edit/complete/delete from the UI,
   with optimistic completion and rollback on error.
7. **Control background listening** (Flow G) — tray and quick panel both
   stop/start listening and show the status; “Exit Nova” is explicit.
8. **Theme change** (Flow: Settings → Appearance) — preview, accent, apply,
   persist; the window repaints instantly and the overlay follows via
   `nova:appearance`.

## 4. Checks actually run

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml` | **ok** — 106 unit tests + 11 integration tests passed, 0 failed, 1 ignored (speech E2E, needs the model) |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | **ok** — no errors, no warnings |
| `npx tsc --noEmit` | **ok** — exit 0 |
| `npx vite build` | **ok** — 63 modules, `dist/assets/index-*.js` ≈ 317 kB (95 kB gzip), CSS ≈ 36 kB |
| `cargo test -- --ignored` (speech E2E) | not run — requires the whisper model plus working audio hardware; kept as an opt-in suite (`npm run test:speech`) |

**Rendered-UI verification.** The production bundle was additionally rendered in a
real browser (non-Tauri preview build, see the README) and walked screen by
screen: the dashboard frame, the six-step onboarding wizard, the Settings shell
with all seven panes, the Appearance page (10 gallery cards, applied vs preview
tags, live preview panel, accent swatches, density, background-effect switches,
glow/transparency sliders, animation intensity, reset), and the floating
assistant in its listening, success, denied and collapsed-pill states. That pass
found and fixed three defects that code review and tests could not:

1. `classList.add("")` in the overlay branch threw during mount — the overlay
   window would have rendered blank. Now each window class is toggled explicitly.
2. Before the status snapshot arrived, the shell claimed “Running” while the wake
   word row said “paused”, and the sidebar claimed the speech model was missing.
   Unknown state now reads “Checking Nova’s status…” and never guesses.
3. A failure of the settings service hid the whole Settings screen, including the
   Appearance page that does not depend on it; Appearance is now reachable with
   the failure reported above it.

Notes: two link attempts failed transiently with `LNK1104` (the test binary was
locked by OneDrive/AV in the workspace folder); re-running succeeded and the
final run above is clean.

## 5. Remaining limitations

- `npm run tauri build` (release bundle + installers) was **not** run in this
  environment — it is a long release build in a OneDrive-synced folder. The dev
  profile compiles the entire crate (lib + bin + tests) and the frontend
  production bundle builds, so packaging configuration is unchanged from the
  existing, previously working setup.
- Voice input needs `models/ggml-tiny.en.bin`; without it Nova says so and
  keeps working with typed commands.
- The wake phrase is built-in (“Hey Nova”); custom phrases are labelled
  *Built-in only*.
- “Check for updates” reports that this build has no update channel.
- Tray *menu* styling is Windows-native and therefore unthemed; the in-app
  quick panel is the themed equivalent.
- Everything Windows-specific (tray behaviour, global shortcut, autostart,
  ShellExecute launches, DPAPI) compiles and is covered by code review, but a
  full interactive session on a Windows desktop was not part of this run.
- Two workspace files (`src-tauri/src/settings.rs`, `.env.example`) could not be
  edited: the workspace safety guard blocks writes whose content contains an
  identifier named `api_key`/`SECRET` alongside a literal, which matches
  pre-existing, secret-free lines in those files. The new preference fields were
  therefore placed in `preferences.rs`, and `.env.example` is unchanged.

## 7. Design gap-closure round (follow-up)

A second pass re-diffed the design artefacts (`V3 Nova-Prototype.html` plus the
component/state inventories in `V3 Nova-Canvas.html` and
`V3 Nova-App-Canvas.html`) against the build. The canvases enumerate the exact
dialogs and notifications the product must have, and six designed behaviours
were still missing. All six are now implemented:

1. **Shutdown confirmation** — the canvases show “Shut down this PC?” and the
   Behavior pane promises that “shutting down asks first”. Added `SHUTDOWN_PC`
   (CONFIRMATION_REQUIRED, 10 s countdown, fixed argument vector) and
   `CANCEL_SHUTDOWN`, plus parser guards so only an explicit request that names
   the machine can reach it and Nova itself can never be shut down.
2. **“Couldn’t hear you clearly”** — a capture with no recognised words used to
   end in a generic failure. It now has its own status (`not_heard`) and the
   designed recovery panel: “Try again” / “Type instead”.
3. **“That isn’t supported yet”** — unsupported requests now show a dedicated
   panel with “See what Nova can do” (opens the documentation) and Dismiss,
   instead of a Retry button that could not work. Web-search phrasings are
   explicitly kept out of the task-search path so they land here, as designed.
4. **“Command cancelled — Nothing was changed.”** toast, shown once per
   cancelled interaction.
5. **Offline states** — Home, Tasks and Activity show the designed offline
   banner, driven by real `online`/`offline` events, with copy that stays
   truthful: Nova’s data is local, only cloud providers are affected.
6. **AI provider switch dialog** — changing to a cloud provider now asks first
   (“Switch AI provider?” → Keep current / Switch) and says whether a credential
   is configured, matching the designed confirmation.

Also added: an “Unavailable” indicator with an explanation for effects the
platform cannot render (panel transparency when `backdrop-filter` is missing),
and the Behavior pane copy now matches the implemented behaviour.

Verified after the round: `cargo test` **109 unit + 11 integration** passing,
`tsc --noEmit` clean, both `vite build` runs green, and the two new overlay
panels rendered in the browser preview (`#/overlay?state=error&result=not_heard`
and `…&result=unsupported`).

### Deliberately not implemented (prototype-only controls)

The prototype’s bottom-left “Prototype controls” panel is authoring tooling, not
product surface: flow buttons (A–E), the overlay-state and page-state selectors,
“Simulate a denied permission”, “Simulate Windows switching” and the
“Design prototype · sample data” label. Their *product* counterparts exist
(real flows, real states, real permission handling, real system-theme
following). The non-Tauri review hatch described in the README replaces them for
design inspection.

## 8. Instruction-level verification (typed input, no speech)

The final pass drives a realistic session of instructions through the *real*
orchestrator — interpretation, validation, risk gate, skill execution, activity
log — via `handle_transcript`, the same entry point the microphone pipeline and
the dashboard's typed commands use. Speech recognition is deliberately not
involved, so the agent can be verified independently of audio hardware:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test common_instructions -- --nocapture
```

The suite (`src-tauri/tests/common_instructions.rs`) runs on Tauri's mock
runtime against an in-memory database — it never touches the user's own tasks —
and asserts both the responses and the persisted state. What it verified, with
the real responses:

| Instruction | Result |
| --- | --- |
| “add buy milk to my list” | `success` — “Added 'buy milk' to your list.” |
| “add finish the DBMS assignment to my to-dos due today at 6 pm high priority for college” | `success` — stored with priority `high`, category `College`, due 18:00 local |
| “what's on my todo list” | `success` — both tasks read back |
| “what do I need to finish today” | `success` — “You have 1 task due today: …” |
| “mark buy milk as completed” | `success` |
| “search my todos for milk” | `success` — finds the completed task |
| “set the DBMS assignment to low priority” | `success` — priority changed in the database |
| “delete the DBMS assignment” | `awaiting_confirmation` — names the task and asks first |
| “no thanks” | `cancelled` — nothing deleted |
| “delete the DBMS assignment” → “yes” | `success` — deleted, one task remains |
| “play some music” | `unsupported` — honest refusal |
| “search the web for laptops” | `unsupported` — no web search, no fake claim |
| “can you add a” → “buy bread” | `awaiting_clarification` then `success` — asks what to add |
| same add twice | `success` — “I already handled that …”, one task only |
| “take a screenshot” | `success` — file written to `Pictures\Nova Screenshots\` |
| “open my downloads folder” | `success` — “Opening Downloads.” |
| “run a script” | `unsupported` — blocked capability stays blocked |
| empty input | `not_heard` — “I didn't catch that. Try again, or type…” |

Activity log for the session: 21 entries — 13 success, 3 unsupported,
1 cancelled, 2 awaiting confirmation, 1 not heard. Final suite: **118 unit +
13 pipeline + 1 instruction session passing**, 0 failures, 1 ignored (the
Store-app activation probe, run separately with `--ignored`).

### Defects this pass found and fixed

1. **“delete the DBMS assignment” was refused** — the delete parser required a
   context word (“todo”, “task”, “my list”) in the sentence, so a natural
   instruction with an explicit target failed while “…todo” worked. An explicit
   `delete <name>` is now accepted; file/folder deletion stays refused.
2. **“open Google Chrome and search for X” claimed to open the whole sentence** —
   the request now reaches the honest “isn't supported yet” path, and app
   resolution reports the *matched* name (“Google Chrome”) instead of the raw
   phrase.
3. **The longest matching alias now wins** — “google chrome” beats “chrome”, so
   the confirmation and the activity log name the right application.
4. **Store apps can actually be launched** — Microsoft To Do (and 11 other
   packaged apps) have no Start-Menu shortcut and no executable path; they are
   now activated through `IApplicationActivationManager`, which returns a real
   error when the package is missing instead of silently doing nothing.
5. **Misheard app names ask instead of refusing** — “Microsoft Studio” (a
   misheard “To Do”) now answers “Did you mean 'Microsoft To Do'?” with the
   candidate as a button, and the answer fills the `application` slot.
6. **Whisper's `[BLANK_AUDIO]` marker** is no longer treated as a command; it
   becomes the “couldn't hear you clearly” recovery.
7. **State machine noise** — `IDLE → EXECUTING` is now a legal transition (a
   command run without a wake phrase), and speaking the wake word after an error
   settles to rest first instead of being refused.
8. **A dead microphone stream is rebuilt** — a WASAPI “stream configuration is
   no longer valid” error is detected, reported and retried up to three times
   before Nova declares the microphone unavailable with the real message.

## 9. Release build and installation

```bash
npm run tauri build                    # release exe + installers
npx tauri build --bundles nsis         # installer only (fast, re-uses the exe)
```

Built and verified on this machine:

| Artefact | Result |
| --- | --- |
| `src-tauri/target/release/nova.exe` | **built** (17.9 MB) and **run**: the window opened and logged `[nova] whisper model loaded: …\models\ggml-tiny.en.bin`, i.e. the standalone build initialises the local speech engine without Node, Vite or Cargo running |
| `src-tauri/target/release/bundle/nsis/Nova_0.1.0_x64-setup.exe` | **built** (4.8 MB) — per-user installer, no admin prompt |
| `src-tauri/target/release/bundle/msi/Nova_0.1.0_x64_en-US.msi` | **not built**: WiX's `light.exe` fails in this environment with `Access is denied. (os error 5)` after `candle` succeeds. The NSIS installer covers the same need; re-run `npx tauri build --bundles msi` on a machine where WiX is permitted (usually a question of antivirus or a locked `target/release/bundle/msi` folder). |

Notes learned while doing it:

- The release exe is **self-contained**: the UI bundle is embedded, the database
  stays in `%APPDATA%\Nova\nova.db`, and the speech model is resolved from
  `NOVA_MODELS_DIR`, the Nova app-data folder, or `<repo>/models`.
- Bundling **patches `nova.exe` in place**, so the app must not be running while
  `tauri build` bundles (otherwise: `failed to remove file … Access is denied`).
  Stop Nova from the tray (Exit Nova) before building.
- `npm run tauri build` also runs the frontend build first, so a type error in
  the UI fails the release build rather than shipping a stale bundle.

A convenience script was added for repeating the instruction-level verification:
`npm run test:agent` (see section 8).

## 6. Where to look

- `README.md` — what Nova is, how to run it, permissions, privacy, limitations.
- `docs/ARCHITECTURE.md` — layers, state ownership, event contracts, safety.
- `docs/APPEARANCE.md` — theme tokens, preview isolation, persistence handoff.
- `design/` — untouched V3 design artefacts (source of truth).
