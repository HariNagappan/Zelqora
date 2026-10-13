// First-run onboarding (design Screen 6 / Flow D) — six steps, real
// permission checks, a real microphone test and a real first command.

import { useEffect, useState } from "react";

import { Icon } from "./Icon";
import { Badge, Button, Field, LevelMeter, Select } from "./ui";
import { api } from "../services/tauri";
import { useOnboardingStore } from "../stores/useOnboarding";
import { useStatusStore } from "../stores/useStatus";
import { toastError, toastInfo, toastSuccess } from "../stores/useToasts";
import { getLevel } from "../lib/level";
import type { MicrophoneStatus, VoiceInfo } from "../types";

const STEP_LABELS = [
  "Welcome",
  "Microphone permission",
  "Microphone test",
  "Assistant setup",
  "First command",
  "Ready",
];

export default function Onboarding() {
  const { state, open, setOpen, patch, finish } = useOnboardingStore();
  const status = useStatusStore((store) => store.status);
  const refreshStatus = useStatusStore((store) => store.refresh);

  const [step, setStep] = useState(1);
  const [microphone, setMicrophone] = useState<MicrophoneStatus | null>(null);
  const [microphones, setMicrophones] = useState<string[]>([]);
  const [voices, setVoices] = useState<VoiceInfo[]>([]);
  const [heard, setHeard] = useState(false);
  const [testing, setTesting] = useState(false);
  const [commandDraft, setCommandDraft] = useState("add test Nova to my to-dos");
  const [commandResult, setCommandResult] = useState<string | null>(null);
  const [selectedMic, setSelectedMic] = useState("");
  const [busy, setBusy] = useState(false);

  const microphoneVerified = state?.microphone_verified ?? false;
  const firstCommandDone = state?.first_command_done ?? false;

  useEffect(() => {
    if (!open) {
      return;
    }
    void api
      .getMicrophoneStatus()
      .then(setMicrophone)
      .catch(() => setMicrophone({ available: false, permission: "unknown", device: null, message: "Nova couldn't read the microphone status." }));
    void api.listMicrophones().then(setMicrophones).catch(() => setMicrophones([]));
    void api.listVoices().then(setVoices).catch(() => setVoices([]));
  }, [open]);

  // Microphone test sweep: only runs while step 3 is open.
  useEffect(() => {
    if (!open || step !== 3) {
      return;
    }
    setTesting(true);
    setHeard(false);
    void api.startMicrophoneTest();
    const sample = window.setInterval(() => {
      if (getLevel() > 0.02) {
        setHeard(true);
      }
    }, 150);
    return () => {
      window.clearInterval(sample);
      setTesting(false);
      void api.stopMicrophoneTest();
    };
  }, [open, step]);

  if (!open || !state) {
    return null;
  }

  const next = () => setStep((current) => Math.min(STEP_LABELS.length, current + 1));
  const back = () => setStep((current) => Math.max(1, current - 1));

  const recheckMicrophone = async () => {
    try {
      const next = await api.getMicrophoneStatus();
      setMicrophone(next);
      if (next.available) {
        toastSuccess("Microphone available", next.device ?? "Using the system default device.");
      } else {
        toastInfo(
          "Still no microphone",
          next.permission === "denied"
            ? "Allow Nova in Windows Settings → Privacy & security → Microphone."
            : next.message ?? "Plug a microphone in and try again.",
        );
      }
    } catch (error) {
      toastError("Couldn't check the microphone", String(error));
    }
  };

  const runFirstCommand = async () => {
    const text = commandDraft.trim();
    if (!text) {
      return;
    }
    setBusy(true);
    setCommandResult(null);
    try {
      await api.processText(text);
      await patch({ first_command_done: true });
      setCommandResult(text);
      toastSuccess("Command sent", "Nova is handling it now.");
    } catch (error) {
      toastError("Couldn't run that command", String(error));
    } finally {
      setBusy(false);
    }
  };

  const body = () => {
    switch (step) {
      case 1:
        return (
          <>
            <h1 style={{ fontSize: 28 }}>Your PC, just a little easier.</h1>
            <p className="mut">
              Nova runs quietly in the background. Say “Hey Nova” and get small things done without
              reaching for the keyboard.
            </p>
            <div className="obfeat">
              <Icon name="mic" />
              <div>
                <div className="stt">“Hey Nova, add finish my assignment to my to-dos.”</div>
                <div className="std">Tasks, summaries, and opening apps, files and folders.</div>
              </div>
            </div>
            <div className="obfeat">
              <Icon name="shield" />
              <div>
                <div className="stt">Wake-word detection stays on your PC.</div>
                <div className="std">
                  Audio is only processed after the wake phrase, and never stored.
                </div>
              </div>
            </div>
          </>
        );
      case 2:
        return microphone?.available ? (
          <>
            <h2>You’re good to go</h2>
            <p className="mut">
              Nova can reach your microphone. Audio is only analysed after the wake phrase — nothing
              is recorded before that.
            </p>
            <Badge kind="ok" icon="check">
              Microphone available
            </Badge>
          </>
        ) : (
          <>
            <h2>Let Nova hear you</h2>
            <p className="mut">
              Nova needs microphone access to hear “Hey Nova”. Nothing is recorded or sent until you
              speak the phrase.
            </p>
            <div className="conf">
              <div className="ct">If the prompt doesn’t appear</div>
              <div className="std">
                Windows Settings → Privacy &amp; security → Microphone → allow Nova.
              </div>
            </div>
            <div className="row" style={{ gap: 8 }}>
              <Button variant="sec" onClick={() => void api.openMicrophoneSettings()}>
                Open Windows Settings
              </Button>
              <Button variant="gst" icon="refresh" onClick={() => void recheckMicrophone()}>
                Check again
              </Button>
            </div>
          </>
        );
      case 3:
        return (
          <>
            <h2>Test your microphone</h2>
            <div className="fld">
              <label className="lbl" htmlFor="ob-mic">
                Microphone
              </label>
              <Select
                id="ob-mic"
                style={{ maxWidth: 330 }}
                value={selectedMic}
                onChange={(event) => {
                  const value = event.target.value;
                  setSelectedMic(value);
                  void api.updateSettings({ microphone: value });
                  toastInfo("Microphone selected", value || "System default device");
                }}
              >
                <option value="">System default</option>
                {microphones.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </Select>
              <span className="hint">Used for the wake word and your commands.</span>
            </div>
            <LevelMeter running={testing} />
            <div className="row" style={{ gap: 8 }}>
              <Badge kind={heard ? "ok" : "mut"}>
                {heard ? "✓ Heard you clearly" : "Waiting for your voice…"}
              </Badge>
              <Button
                variant="sec"
                small
                onClick={() => {
                  setHeard(false);
                  setTesting(true);
                  void api.startMicrophoneTest();
                }}
              >
                Test again
              </Button>
            </div>
            <p className="hint">
              Say anything — the meter moves with the real input level. Nothing is stored.
            </p>
          </>
        );
      case 4:
        return (
          <>
            <h2>Keep it local — or connect a provider</h2>
            <p className="mut">
              Optional. Nova works fully on-device; a provider adds more natural conversation.
            </p>
            <Field label="Speech-to-text" htmlFor="ob-stt">
              <Select
                id="ob-stt"
                defaultValue={status?.sttProvider ?? "local"}
                onChange={(event) => void api.updateSettings({ stt_provider: event.target.value as "local" | "cloud" })}
              >
                <option value="local">Whisper — on-device (recommended)</option>
                <option value="cloud">Cloud — OpenAI-compatible</option>
              </Select>
            </Field>
            <Field label="Command understanding" htmlFor="ob-llm">
              <Select
                id="ob-llm"
                defaultValue={status?.llmProvider ?? "auto"}
                onChange={(event) =>
                  void api.updateSettings({
                    llm_provider: event.target.value as "auto" | "cloud" | "offline",
                  })
                }
              >
                <option value="auto">Auto — cloud when configured</option>
                <option value="offline">Offline rules only</option>
                <option value="cloud">Cloud only</option>
              </Select>
            </Field>
            {voices.length > 0 ? (
              <Field label="Spoken replies" htmlFor="ob-voice">
                <Select
                  id="ob-voice"
                  defaultValue={""}
                  onChange={(event) => void api.updateSettings({ tts_voice: event.target.value })}
                >
                  <option value="">System default voice</option>
                  {voices.map((voice) => (
                    <option key={voice.id} value={voice.id}>
                      {voice.name}
                    </option>
                  ))}
                </Select>
              </Field>
            ) : null}
            <span className="hint">
              A cloud credential can be added later in Settings → Speech &amp; AI. It is stored
              encrypted and never shown again.
            </span>
          </>
        );
      case 5:
        return (
          <>
            <h2>Try your first command</h2>
            <p className="mut">
              Say: <b style={{ color: "var(--fg)" }}>“Hey Nova, add test Nova to my to-dos.”</b>
            </p>
            {firstCommandDone || commandResult ? (
              <>
                <div className="qbox">“{commandResult ?? "add test Nova to my to-dos"}”</div>
                <div className="row">
                  <span className="cic ok">
                    <Icon name="check" size={18} />
                  </span>
                  <div>
                    <b>Task added — nice.</b>
                    <div className="vs">You’ll find it on your Tasks list.</div>
                  </div>
                </div>
              </>
            ) : (
              <>
                <div className="hint">Can’t speak right now? Type the command instead.</div>
                <div className="row" style={{ gap: 6 }}>
                  <input
                    className="inp"
                    value={commandDraft}
                    aria-label="Command to run"
                    onChange={(event) => setCommandDraft(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        void runFirstCommand();
                      }
                    }}
                  />
                  <Button variant="pri" small disabled={busy || !commandDraft.trim()} onClick={() => void runFirstCommand()}>
                    {busy ? "Sending…" : "Send"}
                  </Button>
                </div>
              </>
            )}
          </>
        );
      case 6:
        return (
          <>
            <h2>You’re all set.</h2>
            <p className="mut">
              Nova now lives in your system tray. Say “Hey Nova” whenever you need it — or click the
              tray icon for quick controls.
            </p>
            <div className="obfeat">
              <Icon name="pause" />
              <div>
                <div className="stt">Pausing isn’t quitting</div>
                <div className="std">
                  Stopping the listener keeps Nova in the tray with your tasks intact. Exit is always
                  at the bottom of the tray menu.
                </div>
              </div>
            </div>
            <div className="obfeat">
              <Icon name="kbd" />
              <div>
                <div className="stt">Keyboard fallback</div>
                <div className="std">
                  Press the push-to-talk shortcut at any time to activate Nova without the wake
                  word. The overlay also accepts typed commands.
                </div>
              </div>
            </div>
          </>
        );
      default:
        return null;
    }
  };

  const primaryAction = () => {
    switch (step) {
      case 2:
        return microphone?.available ? "Continue" : "Check again";
      case 3:
        return "Sounds good";
      case 4:
        return "Save & continue";
      case 6:
        return "Open Nova";
      default:
        return "Continue";
    }
  };

  const onPrimary = async () => {
    switch (step) {
      case 2:
        if (!microphone?.available) {
          await recheckMicrophone();
          return;
        }
        next();
        return;
      case 3:
        await patch({ microphone_verified: heard });
        next();
        return;
      case 4:
        toastInfo("Assistant setup saved", "You can change providers later in Settings.");
        next();
        return;
      case 6:
        await finish();
        await refreshStatus();
        toastSuccess("You’re all set", "Nova lives in the tray — say “Hey Nova” anytime.");
        return;
      default:
        next();
    }
  };

  return (
    <div className="ovl">
      <div className="obc" role="dialog" aria-modal="true" aria-label="Set up Nova">
        <div className="obp">
          {STEP_LABELS.map((label, index) => (
            <i key={label} className={index + 1 <= step ? "on" : ""} />
          ))}
        </div>
        <div className="obstep">
          Step {step} of {STEP_LABELS.length} — {STEP_LABELS[step - 1]}
        </div>
        <div className="obb">{body()}</div>
        <div className="obf">
          {step > 1 ? (
            <Button variant="gst" onClick={back}>
              Back
            </Button>
          ) : (
            <button className="more" onClick={() => setOpen(false)}>
              Not now
            </button>
          )}
          <div className="row" style={{ gap: 8 }}>
            {step === 4 ? (
              <Button
                variant="gst"
                onClick={async () => {
                  await patch({ providers_skipped: true });
                  toastInfo("Provider step skipped", "Nova keeps working fully on-device.");
                  next();
                }}
              >
                Skip
              </Button>
            ) : null}
            <Button
              variant="pri"
              disabled={step === 3 && !microphone?.available}
              title={
                step === 3 && !microphone?.available
                  ? "A microphone is required for this step"
                  : undefined
              }
              onClick={() => void onPrimary()}
            >
              {primaryAction()}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
