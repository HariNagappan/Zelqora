// Settings (design Screen 5) — a functional subsystem: every control here has a
// defined effect, validates its input, persists through the backend and reports
// the result. Panes: General · Voice activation · Speech & AI · Behavior ·
// Appearance · Privacy · About.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import AppearancePane from "./AppearancePane";
import { Icon, type IconName } from "../components/Icon";
import { confirmAction } from "../components/Dialog";
import {
  Badge,
  Banner,
  Button,
  Card,
  Field,
  LevelMeter,
  Segmented,
  Select,
  TextInput,
  Toggle,
} from "../components/ui";
import { api } from "../services/tauri";
import { useNovaStore } from "../stores/useNova";
import { useOnboardingStore } from "../stores/useOnboarding";
import { useStatusStore } from "../stores/useStatus";
import { toastError, toastInfo, toastSuccess } from "../stores/useToasts";
import { getLevel } from "../lib/level";
import type { BehaviourPrefs, Diagnostics, Settings as SettingsModel, VoiceInfo } from "../types";

type PaneId = "general" | "voice" | "speech" | "behavior" | "appearance" | "privacy" | "about";

const PANES: { id: PaneId; label: string; icon: IconName }[] = [
  { id: "general", label: "General", icon: "set" },
  { id: "voice", label: "Voice activation", icon: "mic" },
  { id: "speech", label: "Speech & AI", icon: "spark" },
  { id: "behavior", label: "Behavior", icon: "tasks" },
  { id: "appearance", label: "Appearance", icon: "moon" },
  { id: "privacy", label: "Privacy", icon: "shield" },
  { id: "about", label: "About", icon: "info" },
];

export default function SettingsPage() {
  const [pane, setPane] = useState<PaneId>("general");
  const [settings, setSettings] = useState<SettingsModel | null>(null);
  const [behaviour, setBehaviour] = useState<BehaviourPrefs | null>(null);
  const [microphones, setMicrophones] = useState<string[]>([]);
  const [voices, setVoices] = useState<VoiceInfo[]>([]);
  const [credential, setCredential] = useState("");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [micTestOpen, setMicTestOpen] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [diagnostics, setDiagnostics] = useState<Diagnostics | null>(null);
  const [diagnosticsBusy, setDiagnosticsBusy] = useState(false);
  const saveTimer = useRef<number | null>(null);

  const status = useStatusStore((store) => store.status);
  const refreshStatus = useStatusStore((store) => store.refresh);
  const nova = useNovaStore();
  const resetOnboarding = useOnboardingStore((store) => store.reset);
  const lastBump = useRef(nova.activityBump);

  const load = useCallback(async () => {
    try {
      const [loadedSettings, loadedBehaviour, mics, voiceList] = await Promise.all([
        api.getSettings(),
        api.getBehaviour(),
        api.listMicrophones(),
        api.listVoices(),
      ]);
      setSettings(loadedSettings);
      setBehaviour(loadedBehaviour);
      setMicrophones(mics);
      setVoices(voiceList);
      setLoadError(null);
    } catch (error) {
      setLoadError(String(error));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // Keep the assistant status in step with commands run elsewhere.
  useEffect(() => {
    if (lastBump.current !== nova.activityBump) {
      lastBump.current = nova.activityBump;
      void refreshStatus();
    }
  }, [nova.activityBump, refreshStatus]);

  const savedToast = useCallback(() => {
    if (saveTimer.current) {
      window.clearTimeout(saveTimer.current);
    }
    saveTimer.current = window.setTimeout(() => {
      toastSuccess("Settings saved", "Changes apply immediately on this PC.");
    }, 420);
  }, []);

  const patchSettings = useCallback(
    async (patch: Partial<SettingsModel>) => {
      setSettings((current) => (current ? { ...current, ...patch } : current));
      try {
        const updated = await api.updateSettings(patch);
        setSettings(updated);
        savedToast();
        void refreshStatus();
      } catch (error) {
        toastError("Couldn't save that setting", String(error));
        void load();
      }
    },
    [load, refreshStatus, savedToast],
  );

  const patchBehaviour = useCallback(
    async (patch: Partial<BehaviourPrefs>) => {
      const next = behaviour ? { ...behaviour, ...patch } : null;
      if (!next) {
        return;
      }
      setBehaviour(next);
      try {
        const saved = await api.updateBehaviour(next);
        setBehaviour(saved);
        savedToast();
      } catch (error) {
        toastError("Couldn't save that setting", String(error));
        void load();
      }
    },
    [behaviour, load, savedToast],
  );

  const runDiagnostics = async () => {
    setDiagnosticsBusy(true);
    try {
      setDiagnostics(await api.getDiagnostics());
    } catch (error) {
      toastError("Diagnostics failed", String(error));
    } finally {
      setDiagnosticsBusy(false);
    }
  };

  const microphoneStatus = nova.microphone;

  const serviceBadge = useMemo(() => {
    if (!microphoneStatus.available) {
      return <Badge kind="er">Mic unavailable</Badge>;
    }
    if (nova.service === "running") {
      return (
        <Badge kind="ok" icon="check">
          Running
        </Badge>
      );
    }
    if (nova.service === "paused") {
      return <Badge kind="wn">Paused</Badge>;
    }
    return <Badge kind="mut">Stopped</Badge>;
  }, [microphoneStatus.available, nova.service]);

  const paneNav = (
    <nav className="snav" aria-label="Settings categories">
      {PANES.map((item) => (
        <button
          key={item.id}
          className={`sni${pane === item.id ? " on" : ""}`}
          aria-current={pane === item.id ? "true" : undefined}
          onClick={() => setPane(item.id)}
        >
          <Icon name={item.icon} />
          {item.label}
        </button>
      ))}
    </nav>
  );

  if (loadError) {
    // Appearance reads its own store, so it stays usable while the rest of the
    // settings cannot be read — with the failure reported honestly above it.
    return (
      <>
        <div className="ph">
          <div>
            <h1>Settings</h1>
            <p className="mut">Nova couldn’t read your settings.</p>
          </div>
        </div>
        <Banner kind="er">
          Nova couldn’t read your settings. <span className="dim">{loadError}</span>
          <button
            className="more"
            style={{ marginLeft: 8, textDecoration: "underline" }}
            onClick={() => void load()}
          >
            Retry
          </button>
        </Banner>
        <div className="set">
          {paneNav}
          <div>
            {pane === "appearance" ? (
              <AppearancePane />
            ) : (
              <Card style={{ margin: 0 }}>
                <div className="cbd" style={{ paddingTop: 16 }}>
                  <p className="mut" style={{ fontSize: 13 }}>
                    This pane is stored by Nova’s local service, which isn’t responding right now.
                    Retry above, or open Appearance, which works independently.
                  </p>
                </div>
              </Card>
            )}
          </div>
        </div>
      </>
    );
  }

  if (!settings || !behaviour) {
    return (
      <>
        <div className="ph">
          <div>
            <h1>Settings</h1>
            <p className="mut">Loading…</p>
          </div>
        </div>
      </>
    );
  }

  return (
    <>
      <div className="ph">
        <div>
          <h1>Settings</h1>
          <p className="mut">Saved automatically — each change shows a confirmation.</p>
        </div>
      </div>

      <div className="set">
        {paneNav}

        <div>
          {pane === "general" ? (
            <Card style={{ margin: 0 }}>
              <Row
                title="Launch Nova when Windows starts"
                detail="Starts in the tray, so the wake word works right after sign-in."
              >
                <Toggle
                  label="Launch Nova when Windows starts"
                  checked={settings.autostart}
                  onChange={(value) => void patchSettings({ autostart: value })}
                />
              </Row>
              <Row
                title="Closing the window keeps Nova listening"
                detail="The window hides to the tray; only Exit Nova stops the assistant."
              >
                <Badge kind="mut">Always on</Badge>
              </Row>
              <Row
                title="Start minimised"
                detail="Launch straight to the tray without opening this window."
              >
                <Toggle
                  label="Start minimised"
                  checked={settings.start_minimized}
                  onChange={(value) => void patchSettings({ start_minimized: value })}
                />
              </Row>
              <Row
                title={
                  <>
                    Assistant status {serviceBadge}
                  </>
                }
                detail={
                  !microphoneStatus.available
                    ? microphoneStatus.message ?? "No microphone found. Check Windows sound settings."
                    : nova.service === "running"
                      ? `Listening for “${status?.wakeWord ?? settings.wake_word}”.`
                      : nova.service === "paused"
                        ? "Wake word is off. Resume whenever you’re ready."
                        : "Nova is stopped. Start it to turn wake-word detection back on."
                }
              >
                <Button
                  variant="sec"
                  small
                  onClick={() => {
                    const next = nova.service === "running" ? "stopped" : "running";
                    void api.setServiceState(next);
                    toastInfo(
                      next === "running" ? "Nova is running" : "Nova stopped",
                      next === "running"
                        ? "Wake-word detection is on."
                        : "Wake-word detection is off and the microphone is released.",
                    );
                  }}
                >
                  {nova.service === "running" ? "Stop Nova" : "Start Nova"}
                </Button>
              </Row>
              <Row
                title="Setup wizard"
                detail="Re-run the microphone test and first-command walkthrough."
              >
                <Button variant="sec" small onClick={() => void resetOnboarding()}>
                  Run setup again
                </Button>
              </Row>
            </Card>
          ) : null}

          {pane === "voice" ? (
            <Card style={{ margin: 0 }}>
              <Row
                title={`Listen for “${status?.wakeWord ?? settings.wake_word}”`}
                detail="Detection runs on-device; audio is never stored."
              >
                <Toggle
                  label="Wake-word detection"
                  checked={settings.wake_enabled}
                  onChange={(value) => void patchSettings({ wake_enabled: value })}
                />
              </Row>
              <Row
                title={
                  <>
                    Wake phrase <span className="stag">Built-in only</span>
                  </>
                }
                detail="Custom phrases aren’t supported yet — the local detector ships with “Hey Nova”."
              >
                <Select aria-label="Wake phrase" value="hey nova" disabled style={{ width: 200 }}>
                  <option value="hey nova">“Hey Nova”</option>
                </Select>
              </Row>
              <Row title="Push-to-talk shortcut" detail="Works even when the wake word is paused.">
                <TextInput
                  aria-label="Push-to-talk shortcut"
                  style={{ width: 200 }}
                  defaultValue={settings.push_to_talk_hotkey}
                  onBlur={(event) => {
                    const value = event.target.value.trim() || "Ctrl+Shift+Space";
                    if (value !== settings.push_to_talk_hotkey) {
                      void patchSettings({ push_to_talk_hotkey: value });
                    }
                  }}
                />
              </Row>
              <Row title="Microphone" detail="Used for the wake word and commands.">
                <div className="row">
                  <Button variant="sec" small icon="mic" onClick={() => setMicTestOpen(true)}>
                    Test microphone
                  </Button>
                  <Select
                    aria-label="Microphone"
                    style={{ width: 200 }}
                    value={settings.microphone ?? ""}
                    onChange={(event) => void patchSettings({ microphone: event.target.value })}
                  >
                    <option value="">System default</option>
                    {microphones.map((name) => (
                      <option key={name} value={name}>
                        {name}
                      </option>
                    ))}
                  </Select>
                </div>
              </Row>
              <Row
                title="Sensitivity"
                detail="Higher picks up quieter speech — lower reduces false wake-ups."
              >
                <div className="row" style={{ gap: 10 }}>
                  <input
                    className="rng"
                    type="range"
                    min={0}
                    max={1}
                    step={0.05}
                    aria-label={`Sensitivity ${Math.round(settings.sensitivity * 100)}%`}
                    value={settings.sensitivity}
                    onChange={(event) =>
                      setSettings({ ...settings, sensitivity: Number(event.target.value) })
                    }
                    onMouseUp={(event) =>
                      void patchSettings({ sensitivity: Number((event.target as HTMLInputElement).value) })
                    }
                    onKeyUp={(event) =>
                      void patchSettings({ sensitivity: Number((event.target as HTMLInputElement).value) })
                    }
                  />
                  <span className="dim" style={{ fontFamily: "var(--fm)", fontSize: 11, width: 38, textAlign: "right" }}>
                    {Math.round(settings.sensitivity * 100)}%
                  </span>
                </div>
              </Row>
              {!microphoneStatus.available ? (
                <Row title="Microphone permission" detail={microphoneStatus.message ?? "No input device is available."}>
                  <div className="row">
                    <Badge kind="er">{microphoneStatus.permission === "denied" ? "Denied" : "Unavailable"}</Badge>
                    <Button variant="sec" small onClick={() => void api.openMicrophoneSettings()}>
                      Open Windows Settings
                    </Button>
                  </div>
                </Row>
              ) : null}
            </Card>
          ) : null}

          {pane === "speech" ? (
            <Card style={{ margin: 0 }}>
              <Row
                title="Speech-to-text"
                detail="On-device keeps audio local. Cloud sends the captured command for processing."
              >
                <Select
                  aria-label="Speech-to-text provider"
                  style={{ width: 240 }}
                  value={settings.stt_provider}
                  onChange={(event) =>
                    void patchSettings({ stt_provider: event.target.value as SettingsModel["stt_provider"] })
                  }
                >
                  <option value="local">Whisper — on-device (default)</option>
                  <option value="cloud">Cloud — OpenAI-compatible</option>
                </Select>
              </Row>
              <Row
                title="AI provider"
                detail="Optional — without one, Nova uses its built-in offline rules."
              >
                <Select
                  aria-label="AI provider"
                  style={{ width: 240 }}
                  value={settings.llm_provider}
                  onChange={(event) => {
                    const next = event.target.value as SettingsModel["llm_provider"];
                    if (next === settings.llm_provider) {
                      return;
                    }
                    // Switching to a cloud provider changes where command text
                    // goes, so the design asks for confirmation first.
                    if (next === "offline") {
                      void patchSettings({ llm_provider: next });
                      return;
                    }
                    const target =
                      next === "cloud"
                        ? `the cloud model (${settings.openai_model || "not set"})`
                        : "the cloud model whenever a credential is configured";
                    confirmAction({
                      title: "Switch AI provider?",
                      icon: "spark",
                      confirmLabel: "Switch",
                      cancelLabel: "Keep current",
                      body: (
                        <p>
                          Future commands will use <b>{target}</b>. Command text is sent to the
                          provider only while it is selected, and the offline rules keep working as
                          a fallback.
                          {!status?.llmConfigured ? (
                            <>
                              {" "}
                              No credential is configured yet, so nothing changes until you add one
                              below.
                            </>
                          ) : null}
                        </p>
                      ),
                      onConfirm: () => void patchSettings({ llm_provider: next }),
                    });
                  }}
                >
                  <option value="auto">Auto — cloud when configured</option>
                  <option value="cloud">Cloud only</option>
                  <option value="offline">Offline rules only</option>
                </Select>
              </Row>
              <Row title="Model" detail="Used when a cloud provider is selected.">
                <TextInput
                  aria-label="AI model"
                  style={{ width: 240 }}
                  defaultValue={settings.openai_model}
                  onBlur={(event) => {
                    const value = event.target.value.trim() || "gpt-4o-mini";
                    if (value !== settings.openai_model) {
                      void patchSettings({ openai_model: value });
                    }
                  }}
                />
              </Row>
              <Row title="Provider base URL" detail="Any OpenAI-compatible endpoint.">
                <TextInput
                  aria-label="Provider base URL"
                  style={{ width: 240 }}
                  defaultValue={settings.openai_base_url}
                  onBlur={(event) => {
                    const value = event.target.value.trim() || "https://api.openai.com/v1";
                    if (value !== settings.openai_base_url) {
                      void patchSettings({ openai_base_url: value });
                    }
                  }}
                />
              </Row>
              <Row
                title="Provider credential"
                detail={
                  status?.llmConfigured
                    ? `Saved and protected with ${status.credentialProtection}. It is never shown again — paste a new value to replace it.`
                    : "Write-only: Nova stores it encrypted and never sends it back to this window."
                }
              >
                <div className="row">
                  {status?.llmConfigured ? <Badge kind="ok" icon="check">Configured</Badge> : <Badge kind="mut">Not set</Badge>}
                  <TextInput
                    type="password"
                    aria-label="Provider credential"
                    style={{ width: 180 }}
                    placeholder="paste to set"
                    value={credential}
                    onChange={(event) => setCredential(event.target.value)}
                  />
                  <Button
                    variant="sec"
                    small
                    disabled={!credential.trim()}
                    onClick={async () => {
                      try {
                        await api.setProviderCredential(credential.trim());
                        setCredential("");
                        await refreshStatus();
                        toastSuccess("Credential saved", "Stored encrypted for your Windows account.");
                      } catch (error) {
                        toastError("Couldn't save the credential", String(error));
                      }
                    }}
                  >
                    Save
                  </Button>
                  {status?.llmConfigured ? (
                    <Button
                      variant="gst"
                      small
                      onClick={async () => {
                        try {
                          await api.clearProviderCredential();
                          await refreshStatus();
                          toastInfo("Credential removed", "Nova now uses the offline rules.");
                        } catch (error) {
                          toastError("Couldn't remove the credential", String(error));
                        }
                      }}
                    >
                      Remove
                    </Button>
                  ) : null}
                </div>
              </Row>
              <Row
                title="Speak responses aloud"
                detail="Short confirmations are read back using a Windows voice."
              >
                <Toggle
                  label="Speak responses aloud"
                  checked={settings.tts_enabled}
                  onChange={(value) => void patchSettings({ tts_enabled: value })}
                />
              </Row>
              <Row title="Voice" detail="Text-to-speech voice for spoken replies.">
                <Select
                  aria-label="Voice"
                  style={{ width: 240 }}
                  value={settings.tts_voice ?? ""}
                  onChange={(event) => void patchSettings({ tts_voice: event.target.value })}
                >
                  <option value="">System default</option>
                  {voices.map((voice) => (
                    <option key={voice.id} value={voice.id}>
                      {voice.name}
                    </option>
                  ))}
                </Select>
              </Row>
              <Row title="Speaking rate" detail="How fast Nova talks.">
                <div className="row" style={{ gap: 10 }}>
                  <input
                    className="rng"
                    type="range"
                    min={0.5}
                    max={2}
                    step={0.05}
                    aria-label="Speaking rate"
                    value={settings.tts_rate}
                    onChange={(event) => setSettings({ ...settings, tts_rate: Number(event.target.value) })}
                    onMouseUp={(event) =>
                      void patchSettings({ tts_rate: Number((event.target as HTMLInputElement).value) })
                    }
                  />
                  <span className="dim" style={{ fontFamily: "var(--fm)", fontSize: 11, width: 38, textAlign: "right" }}>
                    {settings.tts_rate.toFixed(2)}×
                  </span>
                </div>
              </Row>
              <Row title="Speaking volume" detail="Relative to the system volume.">
                <div className="row" style={{ gap: 10 }}>
                  <input
                    className="rng"
                    type="range"
                    min={0}
                    max={1}
                    step={0.05}
                    aria-label="Speaking volume"
                    value={settings.tts_volume}
                    onChange={(event) => setSettings({ ...settings, tts_volume: Number(event.target.value) })}
                    onMouseUp={(event) =>
                      void patchSettings({ tts_volume: Number((event.target as HTMLInputElement).value) })
                    }
                  />
                  <span className="dim" style={{ fontFamily: "var(--fm)", fontSize: 11, width: 38, textAlign: "right" }}>
                    {Math.round(settings.tts_volume * 100)}%
                  </span>
                </div>
              </Row>
              <Row
                title="Offline speech model"
                detail={
                  status?.modelReady
                    ? "Installed — local wake word and transcription are available."
                    : "Not installed. Voice input stays off until the model is downloaded (~75 MB)."
                }
              >
                {status?.modelReady ? (
                  <Badge kind="ok" icon="check">
                    Ready
                  </Badge>
                ) : (
                  <Button
                    variant="sec"
                    small
                    disabled={downloading}
                    onClick={async () => {
                      setDownloading(true);
                      try {
                        const path = await api.downloadModel();
                        await refreshStatus();
                        toastSuccess("Speech model ready", path);
                      } catch (error) {
                        toastError("Download failed", String(error));
                      } finally {
                        setDownloading(false);
                      }
                    }}
                  >
                    {downloading ? "Downloading…" : "Download speech model"}
                  </Button>
                )}
              </Row>
            </Card>
          ) : null}

          {pane === "behavior" ? (
            <Card style={{ margin: 0 }}>
              <Row title="Response verbosity" detail="How much Nova says back after a command.">
                <Segmented
                  label="Response verbosity"
                  value={behaviour.response_verbosity}
                  onChange={(value) => void patchBehaviour({ response_verbosity: value })}
                  options={[
                    { id: "brief", label: "Brief" },
                    { id: "standard", label: "Standard" },
                    { id: "detailed", label: "Detailed" },
                  ]}
                />
              </Row>
              <Row
                title="Confirm before destructive actions"
                detail="Deleting tasks, clearing history and shutting down the PC ask first."
              >
                <Toggle
                  label="Confirm before destructive actions"
                  checked={settings.confirm_destructive}
                  onChange={(value) => void patchSettings({ confirm_destructive: value })}
                />
              </Row>
              <Row
                title="Confirm before opening apps and files"
                detail="Shows a confirmation before Nova opens anything on this PC."
              >
                <Toggle
                  label="Confirm before opening apps and files"
                  checked={behaviour.confirm_open}
                  onChange={(value) => void patchBehaviour({ confirm_open: value })}
                />
              </Row>
              <Row
                title="Show the overlay after each command"
                detail="Off — commands typed in the dashboard run quietly."
              >
                <Toggle
                  label="Show the overlay after each command"
                  checked={behaviour.overlay_after_command}
                  onChange={(value) => void patchBehaviour({ overlay_after_command: value })}
                />
              </Row>
              <Row
                title="Keep the overlay expanded between commands"
                detail="Off — the overlay collapses to a small pill when idle."
              >
                <Toggle
                  label="Keep the overlay expanded between commands"
                  checked={behaviour.overlay_keep_expanded}
                  onChange={(value) => void patchBehaviour({ overlay_keep_expanded: value })}
                />
              </Row>
              <Row
                title="Show a notification when a command finishes"
                detail="In-app notifications for commands run while this window is closed."
              >
                <Toggle
                  label="Show a notification when a command finishes"
                  checked={behaviour.notifications_enabled}
                  onChange={(value) => void patchBehaviour({ notifications_enabled: value })}
                />
              </Row>
              <Row
                title="Floating assistant"
                detail="Show the always-on-top overlay window."
              >
                <Toggle
                  label="Floating assistant"
                  checked={settings.overlay_enabled}
                  onChange={(value) => void patchSettings({ overlay_enabled: value })}
                />
              </Row>
            </Card>
          ) : null}

          {pane === "appearance" ? <AppearancePane /> : null}

          {pane === "privacy" ? (
            <Card style={{ margin: 0 }}>
              <Row
                title="Background listening"
                detail="Nova listens only for the wake phrase. Audio is processed on-device and never stored."
              >
                <Badge kind={settings.wake_enabled ? "ok" : "mut"}>
                  {settings.wake_enabled ? "On-device" : "Off"}
                </Badge>
              </Row>
              <Row title="Processing" detail="Where each stage of a command runs.">
                <div className="row" style={{ gap: 6 }}>
                  <span className="chip">Wake word · local</span>
                  <span className="chip">
                    Speech · {settings.stt_provider === "cloud" ? "cloud (command only)" : "local"}
                  </span>
                  <span className="chip">
                    Understanding · {status?.llmConfigured ? "cloud + rules" : "rules (local)"}
                  </span>
                </div>
              </Row>
              <Row title="Command history" detail="Commands are kept on this PC so you can review them.">
                <Select
                  aria-label="History retention"
                  style={{ width: 200 }}
                  value={String(behaviour.history_retention_days)}
                  onChange={(event) => void patchBehaviour({ history_retention_days: Number(event.target.value) })}
                >
                  <option value="30">Keep for 30 days</option>
                  <option value="90">Keep for 90 days</option>
                  <option value="365">Keep for 1 year</option>
                  <option value="0">Keep forever</option>
                </Select>
              </Row>
              <Row
                title="Clear command history"
                detail="Removes every transcript and result from this device. Tasks are not affected."
              >
                <Button
                  variant="dst"
                  small
                  onClick={() =>
                    confirmAction({
                      title: "Clear command history?",
                      icon: "trash",
                      destructive: true,
                      confirmLabel: "Clear history",
                      body: <p>All transcripts, intents and results will be removed. Tasks are not affected.</p>,
                      onConfirm: async () => {
                        const removed = await api.clearActivityHistory();
                        toastSuccess("History cleared", `${removed} entries removed.`);
                      },
                    })
                  }
                >
                  Clear history
                </Button>
              </Row>
              <Row
                title="Microphone permission"
                detail={
                  microphoneStatus.available
                    ? "Nova currently has access to your microphone."
                    : microphoneStatus.message ?? "Nova can’t reach a microphone right now."
                }
              >
                <div className="row">
                  <Badge kind={microphoneStatus.available ? "ok" : "er"}>
                    {microphoneStatus.available ? "Granted" : microphoneStatus.permission === "denied" ? "Denied" : "Unavailable"}
                  </Badge>
                  <Button variant="sec" small onClick={() => void api.openMicrophoneSettings()}>
                    Open Windows Settings
                  </Button>
                </div>
              </Row>
              <Row
                title="Stored credentials"
                detail={status?.credentialProtection ?? "Provider credentials are encrypted for your Windows account."}
              >
                <Badge kind="ok" icon="shield">
                  Encrypted
                </Badge>
              </Row>
            </Card>
          ) : null}

          {pane === "about" ? (
            <Card style={{ margin: 0 }}>
              <Row
                title="Version"
                detail={`Nova ${status?.version ?? "0.1.0"} · Tauri 2 desktop build`}
              >
                <Button
                  variant="sec"
                  small
                  onClick={async () => {
                    try {
                      const info = await api.checkForUpdates();
                      toastInfo(info.available ? "Update available" : "You’re up to date", info.message);
                    } catch (error) {
                      toastError("Couldn’t check for updates", String(error));
                    }
                  }}
                >
                  Check for updates
                </Button>
              </Row>
              <Row
                title="Diagnostics"
                detail="Checks the microphone, speech engine, local database and provider setup."
              >
                <Button variant="sec" small disabled={diagnosticsBusy} onClick={() => void runDiagnostics()}>
                  {diagnosticsBusy ? "Running…" : "Run diagnostics"}
                </Button>
              </Row>
              {diagnostics ? (
                <div style={{ padding: "4px 16px 16px" }}>
                  <div className="dgrid">
                    {diagnostics.checks.map((check) => (
                      <div key={check.name} style={{ display: "contents" }}>
                        <span className="dk">{check.name}</span>
                        <span>
                          <Icon
                            name={check.ok ? "check" : "alert"}
                            size={12}
                            className={check.ok ? "ok" : "er"}
                          />{" "}
                          {check.detail}
                        </span>
                      </div>
                    ))}
                  </div>
                  <p className="dim" style={{ fontSize: 11.5, marginTop: 10 }}>
                    Version {diagnostics.version}
                    {diagnostics.schemaVersion != null ? ` · database schema v${diagnostics.schemaVersion}` : ""}
                    {diagnostics.pushToTalk ? ` · push-to-talk ${diagnostics.pushToTalk}` : ""}
                  </p>
                </div>
              ) : null}
              <Row title="Documentation & logs" detail="Guides for voice commands and provider setup.">
                <div className="row">
                  <Button
                    variant="gst"
                    small
                    onClick={async () => {
                      try {
                        toastInfo("Opening documentation…", await api.openDocumentation());
                      } catch (error) {
                        toastError("Couldn’t open the documentation", String(error));
                      }
                    }}
                  >
                    Documentation
                  </Button>
                  <Button
                    variant="gst"
                    small
                    onClick={async () => {
                      try {
                        toastInfo("Opening logs folder…", await api.openLogsFolder());
                      } catch (error) {
                        toastError("Couldn’t open the folder", String(error));
                      }
                    }}
                  >
                    Open data folder
                  </Button>
                </div>
              </Row>
            </Card>
          ) : null}
        </div>
      </div>

      {micTestOpen ? <MicrophoneTestDialog onClose={() => setMicTestOpen(false)} /> : null}
    </>
  );
}

function Row({
  title,
  detail,
  children,
}: {
  title: React.ReactNode;
  detail: string;
  children: React.ReactNode;
}) {
  return (
    <div className="srow">
      <div className="st">
        <div className="stt">{title}</div>
        <div className="std">{detail}</div>
      </div>
      {children}
    </div>
  );
}

/** Microphone test: live level meter driven by real audio. */
function MicrophoneTestDialog({ onClose }: { onClose: () => void }) {
  const [running, setRunning] = useState(true);
  const [heard, setHeard] = useState(false);
  const peak = useRef(0);

  const startSweep = () => {
    peak.current = 0;
    setHeard(false);
    setRunning(true);
    void api.startMicrophoneTest();
  };

  useEffect(() => {
    void api.startMicrophoneTest();
    const sample = window.setInterval(() => {
      peak.current = Math.max(peak.current, getLevel());
      if (peak.current > 0.02) {
        setHeard(true);
      }
    }, 150);
    return () => {
      window.clearInterval(sample);
      void api.stopMicrophoneTest();
    };
  }, []);

  return (
    <div className="dlgHost">
      <div className="dlg" role="dialog" aria-modal="true" aria-label="Test microphone">
        <h3>
          <Icon name="mic" />
          Test microphone
        </h3>
        <p>Say something like “testing, one two three”.</p>
        <LevelMeter running={running} />
        <div className="row" style={{ gap: 8, marginTop: 8 }}>
          <Badge kind={heard ? "ok" : "mut"}>
            {heard ? "✓ Heard you clearly" : "Listening…"}
          </Badge>
          <span className="dim" style={{ fontSize: 11.5 }}>
            Levels come straight from the microphone — nothing is recorded.
          </span>
        </div>
        <div className="dacts">
          <Button variant="sec" onClick={startSweep}>
            Test again
          </Button>
          <Button variant="pri" onClick={onClose}>
            Done
          </Button>
        </div>
      </div>
    </div>
  );
}
