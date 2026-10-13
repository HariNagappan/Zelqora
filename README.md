# Nova — a voice-controlled personal computer agent for Windows

Nova runs in the background like a small voice assistant: say **“Hey Nova”**,
speak a command, and Nova understands it, performs the action on your PC, and
answers out loud. The dashboard, the floating assistant and the whole
appearance system implement the V3 design artefacts in `./design`.

```
Microphone → Wake word (local) → Speech-to-text → Agent / intent
           → Command registry → Permission & validation → Skill executor
           → Result → Text-to-speech → Floating overlay + dashboard
```

**Privacy-first by design:** the wake word and speech recognition run locally
(whisper.cpp). Microphone audio is only processed after the wake phrase — and
never uploaded unless you explicitly select a cloud provider. The optional
provider credential is encrypted at rest with Windows DPAPI.

---

## What is implemented

**Application surfaces**

- **Dashboard (Home)** — greeting, assistant status (wake word, microphone,
  speech provider, understanding), “Test voice activation”, today’s tasks and
  recent commands, with loading / empty / error states.
- **Tasks** — pending / completed / overdue views, search, sorting, due dates,
  priorities, categories, voice-created marks, create + edit modal, guarded
  deletion with undo, inline completion with optimistic update.
- **Activity** — what Nova heard, understood, did and answered, with status
  filters (success / failed / cancelled), search, expandable details, retry for
  retryable failures, and history clearing.
- **Settings** — General, Voice activation, Speech & AI, Behavior, Appearance,
  Privacy, About. Every control has a defined effect and persists immediately.
- **Appearance** — ten themes with miniature previews of the real UI, an
  isolated live preview, accent colours, density, background effects, glow and
  transparency, plus apply / cancel / reset flows.
- **Onboarding** — six connected steps: welcome, microphone permission,
  microphone test with a live meter, optional provider setup, first voice
  command, and the tray/ready summary.
- **Floating assistant (overlay)** — idle pill, wake detected, listening with a
  live waveform, transcribing, thinking, awaiting confirmation, awaiting
  clarification with candidate buttons, executing, success, error with retry,
  microphone denied, cancelled and typed-input fallback.
- **Quick-status panel + native tray** — the tray menu mirrors the design
  (status line, open, stop/start listening, tasks, activity, settings,
  microphone help, exit) and the in-app panel has the same entries.

**Voice + agent**

- Local wake-word detection (whisper.cpp) with configurable sensitivity,
  push-to-talk shortcut (`Ctrl+Shift+Space`) and manual activation.
- Local or cloud speech-to-text (only the post-wake command segment is sent).
- Offline rule-based understanding **plus** optional OpenAI-compatible tool
  calling; the model can only emit commands from the registry.
- A guarded state machine (`IDLE` → `WAKE_DETECTED` → `LISTENING_FOR_COMMAND` →
  `TRANSCRIBING` → `PROCESSING` → `EXECUTING` → `SUCCESS`/`ERROR`, plus
  `AWAITING_CONFIRMATION`, `AWAITING_CLARIFICATION`, `MICROPHONE_DENIED`, …).
  Illegal transitions are refused, and every state can recover, so the
  assistant can never wedge itself.
- Confirmation gate for destructive actions (and optionally for opening apps,
  files and folders), clarification questions for ambiguous task references,
  duplicate-submission protection, timeouts and honest failure reporting.

**Command registry**

| Intent | Risk | Description |
| --- | --- | --- |
| `ADD_TODO` | SAFE | Add a task, optionally with due date, priority, category |
| `LIST_TODOS` | SAFE | List open tasks |
| `SUMMARIZE_TODOS` | SAFE | “What do I need to finish today?” (today/tomorrow/week/overdue/all) |
| `COMPLETE_TODO` | SAFE | Complete a task (fuzzy match, asks when ambiguous) |
| `EDIT_TODO` | SAFE | Rename / re-prioritise / reschedule / re-categorise a task |
| `DELETE_TODO` | CONFIRMATION | Delete a task — asks first |
| `CLEAR_COMPLETED_TODOS` | CONFIRMATION | Remove completed tasks — asks first |
| `SEARCH_TODOS` | SAFE | Search tasks |
| `OPEN_APPLICATION` | SAFE | Launch an installed app (Start-Menu index + curated aliases) |
| `OPEN_FILE` | SAFE | Open a document (executables/scripts refused) |
| `OPEN_FOLDER` | SAFE | Open a known folder or absolute path |
| `TAKE_SCREENSHOT` | SAFE | Save a screenshot to `Pictures\Nova Screenshots` |
| `SHUTDOWN_PC` | CONFIRMATION | Shut the PC down after a 10 s countdown — always asks first |
| `CANCEL_SHUTDOWN` | SAFE | Cancel a shutdown that is still counting down |
| `RUN_SCRIPT` | **BLOCKED** | Refused — scripts are never executed |
| `EXECUTE_SHELL_COMMAND` | **BLOCKED** | Refused — no shell execution |

Example phrasings that all resolve to `ADD_TODO`:
“Add buy milk to my list” · “Remind me that I need to buy milk” ·
“Put buy milk on my todo list” · “I need to buy milk, add it to my tasks” ·
“Add finish my DBMS assignment to my to-dos due today at 6 pm”.

---

## Requirements

- Windows 10/11 with WebView2 (preinstalled on Windows 11)
- **Node.js 18+** and npm
- **Rust** (stable) with the MSVC toolchain
- Visual Studio Build Tools with the *Desktop development with C++* workload
- CMake — needed to compile whisper.cpp
- (whisper-rs also needs `libclang` at build time; see *Troubleshooting*)

Disk: ~1 GB for build artefacts, plus ~80 MB for the speech model.

## Quick start (development)

```bash
npm install
npm run models        # downloads ggml-tiny.en.bin (~75 MB) — wake word + local STT
npm run tauri dev
```

Without the model, Nova still works from the dashboard (typed commands + tray),
and Settings → Speech & AI offers a **Download speech model** button.

## Production build

```bash
npm run build         # type-check + production UI bundle (dist/)
npm run tauri build   # release nova.exe + Windows installers
```

Output (no Node.js, Vite or Rust needed to *run* it — WebView2 is preinstalled on
Windows 11 and comes with Windows 10 updates):

| Artefact | Path |
| --- | --- |
| Standalone executable | `src-tauri/target/release/nova.exe` |
| NSIS installer | `src-tauri/target/release/bundle/nsis/Nova_<version>_x64-setup.exe` |
| MSI installer | `src-tauri/target/release/bundle/msi/Nova_<version>_x64_en-US.msi` |

`nova.exe` runs on its own: the UI is embedded, the SQLite database lives in
`%APPDATA%\Nova\nova.db`, and the speech model is looked up in `NOVA_MODELS_DIR`,
the Nova app-data folder, or `<repo>/models`. The dashboard's tabs need nothing
else; the tray, global shortcut and auto-start work with the installed build.

After installing, tick **Settings → General → “Launch Nova when Windows starts”**
if you want the wake word available right after sign-in (it starts hidden in the
tray).

Still needed for *building*: Rust with the MSVC toolchain, Visual Studio Build
Tools (Desktop development with C++), CMake, and `libclang` for whisper-rs.

## Testing

```bash
npm test              # cargo test — unit + integration suites
npm run test:agent    # scripted session of real instructions, with the responses printed
npm run check         # cargo check
npm run build         # tsc --noEmit + vite build (also type-checks the UI)
npm run test:speech   # optional: whisper speech E2E on the "Hey Nova" fixture
```

`npm run test:agent` is the quickest way to see the agent work end to end without
a microphone: it feeds a realistic session (add · list · summarise · complete ·
search · edit · delete-with-confirmation · cancel · unsupported · clarification ·
duplicate · screenshot · open folder · blocked) through the real orchestrator and
prints every instruction with Nova's answer, then asserts the stored state. It
runs against a throwaway database, so your own tasks are untouched.

### Reviewing the UI without the desktop shell

```bash
npx vite build --base=./ --outDir=.zwork/ui-preview
# then open .zwork/ui-preview/index.html in a browser:
#   #/            dashboard        #/tasks   tasks
#   #/activity    history          #/settings settings (incl. Appearance)
#   #/overlay?state=listening     floating assistant in any state
#   (states: mini, idle, wake, listening, transcribing, processing, confirm,
#    executing, success, error, denied, cancelled, text)
```

Outside the Tauri shell the UI falls back to defaults and reports backend
failures honestly, which makes this a useful design-fidelity and regression
check. The preview hatch is inert inside the real application.

Covered by tests: offline parser fixtures (including due-date / priority /
category slots, edit and summary intents), task reference resolution, command
validation and risk rules, confirmation parsing, the voice state machine and its
recovery rules, appearance/theme defaults and validation, behaviour clamping,
database migrations (including a v1 → v2 upgrade that preserves existing rows),
task CRUD with due dates / priorities / categories, activity filtering and
retention, at-rest protection of the provider credential, and two end-to-end
suites (orchestrator on Tauri’s mock runtime, plus the optional speech suite).

## Configuration

Settings live in the app and are stored in SQLite
(`%APPDATA%\Nova\nova.db`). A `.env` file (see `.env.example`) provides
fallbacks and CI-friendly overrides:

| Variable | Purpose |
| --- | --- |
| `OPENAI_API_KEY` | Fallback provider credential (the Settings UI takes precedence) |
| `OPENAI_BASE_URL` | OpenAI-compatible base URL (default `https://api.openai.com/v1`) |
| `OPENAI_MODEL` | Model name (default `gpt-4o-mini`) |
| `NOVA_MODELS_DIR` | Where to look for / download the whisper model |

Preferences with their own storage rows: `appearance` (theme, accent, density,
effects), `behaviour` (verbosity, confirmations, overlay behaviour, retention,
notifications) and `onboarding` (completion + step facts).

## Privacy model

| Stage | Default | Cloud option |
| --- | --- | --- |
| Wake-word detection | **Local** (whisper.cpp) | never cloud |
| Speech-to-text | **Local** (whisper.cpp) | optional (command audio only) |
| Command understanding | **Offline rules** | optional (OpenAI-compatible) |
| Provider credential | not stored | stored with Windows DPAPI (current user) |

Nova never streams the microphone. Audio is only analysed after the wake word
(or push-to-talk), and only the captured command segment is ever transcribed.
Typed commands never leave the machine unless a cloud provider is selected.

**Process execution.** Nova runs exactly one external process, and only after
the user confirms it: the shutdown countdown (`shutdown /s /t <n>`, plus
`shutdown /a` to abort). The argument vector is fixed in code — nothing from a
transcript, model or settings is interpolated into it — no shell is involved,
and there is no command that accepts free-form command lines. Everything else
launches through `ShellExecuteW` with the “open” verb, which the file/script
extension guard constrains.

Privacy details worth knowing:

- The credential is written to the database as a DPAPI blob; the UI can only
  *set* or *remove* it, never read it back (`get_settings` blanks the field).
- Command history retention (30 / 90 / 365 days / forever) is enforced at
  startup and whenever the preference changes.
- Nothing is recorded while Nova is idle: levels are only emitted while a
  command is being captured or the microphone test is running.

## Project layout

```
├── design/                V3 design artefacts (prototypes + canvas) — source of truth
├── docs/
│   ├── ARCHITECTURE.md    Layers, state ownership, event contracts, persistence
│   └── APPEARANCE.md      Theme tokens, preview isolation, persistence handoff
├── migrations/            SQL migrations (embedded at build time)
├── models/                whisper model (git-ignored; `npm run models`)
├── scripts/               model downloader + icon generator
├── src/                   React dashboard + overlay
│   ├── components/        shell, overlay, dialogs, toasts, ambient canvas, primitives
│   ├── pages/             Home, Tasks, Activity, Settings, Appearance
│   ├── services/          typed Tauri command/event wrappers
│   ├── stores/            zustand state (voice, tasks, appearance, prefs, toasts)
│   ├── theme/             theme catalogue + token maths
│   └── lib/               formatting, level store, window helpers
└── src-tauri/
    ├── capabilities/      Tauri v2 permissions
    └── src/
        ├── agent/         orchestrator, context, LLM providers, slot extraction
        ├── appearance.rs  theme catalogue + customization model
        ├── audio/         capture engine, VAD, resampler, level meter
        ├── commands/      registry, validator, executor
        ├── database/      SQLite access + migrations runner
        ├── preferences.rs behaviour preferences
        ├── secrets.rs     DPAPI protection for the provider credential
        ├── skills/        todo, windows
        ├── stt/           whisper worker, cloud STT, model management
        ├── tts/           Windows text-to-speech
        └── runtime.rs     tray, window/overlay management, event loop
```

## Known limitations

- **Voice needs the local model.** Without `models/ggml-tiny.en.bin` the wake
  word and local transcription are unavailable; Nova says so in the UI instead
  of pretending otherwise.
- **Wake phrases are built-in.** The detector ships with “Hey Nova”; custom
  phrases are marked *Built-in only* in Settings.
- **Check for updates** reports that this build has no update channel — it does
  not claim an update exists.
- **Hybrid sleep / device changes** can drop the audio stream; Nova reports
  `MICROPHONE_UNAVAILABLE` and recovers when the device returns.
- **Cloud provider behaviour** (latency, model quality) depends on the
  configured endpoint; the offline rules keep working as a fallback.
- **Tray menu styling** is Windows-native and therefore not themed; the in-app
  quick panel is the themed equivalent.
- Everything Windows-specific (tray, global shortcut, autostart, ShellExecute)
  is verified by compilation and code review; a full interactive run requires a
  Windows desktop session.

## Troubleshooting

- **“Wake model not installed”** — run `npm run models`, or use
  Settings → Speech & AI → *Download speech model*.
- **Nova doesn’t hear me** — check Windows microphone permissions
  (Settings → Privacy & security → Microphone) and the device selected in
  Settings → Voice activation; use *Test microphone* to confirm input.
- **False wake-ups** — lower the sensitivity slider a little; the detector uses
  fuzzy matching to tolerate ASR variation.
- **`libclang` or CMake errors while building** — whisper-rs needs them at build
  time only: install LLVM (and CMake) or point `LIBCLANG_PATH` at a
  `libclang.dll`. If CMake picks an unsupported Visual Studio generator, set
  `CMAKE_GENERATOR="Visual Studio 17 2022"`.
- **Building inside a OneDrive-synced folder** — works, but sync + antivirus can
  slow builds; exclude `src-tauri/target/` from sync.

## Roadmap

New capabilities plug into the same registry/skill seam without touching the
agent core: `BrowserSkill`, `GmailSkill`, `CalendarSkill`, `GitHubSkill`,
`DiscordSkill`, `VSCodeSkill`, `SpotifySkill` — each registering intents with
their own risk levels and design-time settings panes.
