// Settings → Appearance (design: theme gallery, live preview, customization).
//
// Preview vs applied is enforced structurally: the preview renders inside its
// own `data-th` scope and every control here writes to the preview store, never
// to application state. Nothing in the preview scope creates tasks, changes
// settings, or touches the voice pipeline.

import { useEffect, useMemo, useRef, useState } from "react";

import { Icon } from "../components/Icon";
import { confirmAction } from "../components/Dialog";
import { Badge, Button, Card, Emblem, Segmented, Toggle, Waveform } from "../components/ui";
import { activeAppearance, useAppearanceStore } from "../stores/useAppearance";
import { toastError, toastInfo, toastSuccess } from "../stores/useToasts";
import { applyThemeToElement } from "../theme/themes";
import { ACCENTS, DENSITIES, INTENSITIES, THEMES as THEME_LIST, rgba, themeName } from "../theme/themes";
import type { Effects, ThemeId } from "../types";

export default function AppearancePane() {
  const applied = useAppearanceStore((state) => state.applied);
  const preview = useAppearanceStore((state) => state.preview);
  const systemIsDark = useAppearanceStore((state) => state.systemIsDark);
  const error = useAppearanceStore((state) => state.error);
  const startPreview = useAppearanceStore((state) => state.startPreview);
  const cancelPreview = useAppearanceStore((state) => state.cancelPreview);
  const applyPreview = useAppearanceStore((state) => state.applyPreview);
  const applyTheme = useAppearanceStore((state) => state.applyTheme);
  const setAccent = useAppearanceStore((state) => state.setAccent);
  const setDensity = useAppearanceStore((state) => state.setDensity);
  const setEffect = useAppearanceStore((state) => state.setEffect);
  const setIntensity = useAppearanceStore((state) => state.setIntensity);
  const resetCustomization = useAppearanceStore((state) => state.resetCustomization);
  const resetAll = useAppearanceStore((state) => state.resetAll);

  const [customizeOpen, setCustomizeOpen] = useState(true);
  const [busy, setBusy] = useState(false);
  const previewRef = useRef<HTMLDivElement | null>(null);

  // Some Windows/WebView2 configurations cannot blur translucent surfaces.
  // Rather than pretending, the control is marked unavailable and says why.
  const blurSupported =
    typeof CSS === "undefined" || typeof CSS.supports !== "function"
      ? true
      : CSS.supports("backdrop-filter", "blur(4px)");

  const active = activeAppearance({ applied, preview });
  const previewing = preview !== null;
  const shownTheme = active.theme;

  // The preview panel is scoped with the *preview* values, while the rest of
  // the application keeps using the applied theme.
  useEffect(() => {
    if (!previewRef.current) {
      return;
    }
    applyThemeToElement(previewRef.current, {
      theme: active.theme,
      accent: active.accent,
      effects: active.effects,
      systemIsDark,
    });
  }, [active, systemIsDark]);

  const apply = async (theme: ThemeId) => {
    setBusy(true);
    try {
      if (previewing && preview?.theme === theme) {
        await applyPreview();
      } else {
        await applyTheme(theme);
      }
      if (useAppearanceStore.getState().error) {
        throw new Error(useAppearanceStore.getState().error ?? "unknown error");
      }
      toastSuccess("Theme applied", `“${themeName(theme)}” is saved for your next launch.`);
    } catch (err) {
      toastError("Couldn't apply the theme", String(err));
    } finally {
      setBusy(false);
    }
  };

  const customizing = useMemo(
    () => (preview ? preview.customized : applied.customized),
    [applied.customized, preview],
  );

  return (
    <>
      <div className="ph" style={{ marginBottom: 10 }}>
        <div>
          <h1 style={{ fontSize: 22 }}>Appearance</h1>
          <p className="mut">Themes, accents, density and background effects — preview before you apply.</p>
        </div>
        <div className="row" style={{ gap: 8 }}>
          {previewing ? (
            <span className="ap-tag pvt">Previewing — not applied</span>
          ) : applied.theme === "system" ? (
            <span className="ap-tag">System controlled</span>
          ) : (
            <span className="ap-tag appl">Applied</span>
          )}
          {customizing && !previewing ? <span className="ap-tag cust">Customized</span> : null}
        </div>
      </div>

      {error ? (
        <div className="bnr er">
          <Icon name="alert" />
          <span className="grow">{error}</span>
        </div>
      ) : null}

      <div className="cap" style={{ margin: "2px 0 10px" }}>
        Theme gallery · {THEME_LIST.length} themes
      </div>
      <div className="ap-grid">
        {THEME_LIST.map((theme) => {
          const isApplied = applied.theme === theme.id;
          const isPreviewing = preview?.theme === theme.id;
          return (
            <div
              key={theme.id}
              className={`ap-card${isApplied ? " applied" : ""}${isPreviewing ? " pv" : ""}`}
              data-th={theme.id === "system" ? (systemIsDark ? "nova" : "light") : theme.id}
              role="button"
              tabIndex={0}
              aria-label={`Preview theme ${theme.name}`}
              onClick={() => startPreview(theme.id)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  startPreview(theme.id);
                }
              }}
            >
              <MiniPreview />
              <div className="ap-name">
                {theme.name}
                {isApplied ? <span className="ap-tag appl">Applied</span> : null}
                {isPreviewing ? <span className="ap-tag pvt">Preview</span> : null}
                {isApplied && applied.customized ? <span className="ap-tag cust">Customized</span> : null}
              </div>
              <div className="ap-desc">{theme.description}</div>
              <div className="ap-acts">
                <Button
                  variant="sec"
                  small
                  onClick={(event) => {
                    event.stopPropagation();
                    startPreview(theme.id);
                  }}
                >
                  Preview
                </Button>
                <Button
                  variant="pri"
                  small
                  disabled={busy}
                  onClick={(event) => {
                    event.stopPropagation();
                    void apply(theme.id);
                  }}
                >
                  Apply
                </Button>
              </div>
            </div>
          );
        })}
      </div>

      <div className="row" style={{ justifyContent: "space-between", margin: "18px 0 8px" }}>
        <div className="cap">
          Live preview — {themeName(shownTheme)}{" "}
          <span className={`ap-tag ${previewing ? "pvt" : "appl"}`} style={{ marginLeft: 8 }}>
            {previewing ? "Previewing — not applied" : "Currently applied"}
          </span>
          {customizing ? (
            <span className="ap-tag cust" style={{ marginLeft: 6 }}>
              Customized
            </span>
          ) : null}
        </div>
        <span className="dim" style={{ fontSize: 11 }}>
          Isolated — testing a theme never touches your tasks, settings, or voice.
        </span>
      </div>

      <div className="pvx">
        <div ref={previewRef} data-th="nova" style={{ display: "contents" }}>
          <div className="pv-inner">
            <div className="pv-rail">
              <b>Nova</b>
              <i className="on">Dashboard</i>
              <i>Tasks</i>
              <i>Activity</i>
              <i>Settings</i>
            </div>
            <div className="pv-main">
              <div className="pv-cap">Scenario — “Add a task to finish my DBMS revision”</div>
              <div className="vo" style={{ position: "relative", width: "100%" }}>
                <div className="vh">
                  <Emblem size={24} state="lis" />
                  <div className="grow">
                    <div className="vn">Nova</div>
                    <div className="vst">Listening</div>
                  </div>
                  <span className="ibt" aria-hidden="true">
                    <Icon name="x" />
                  </span>
                </div>
                <div className="vb">
                  <Waveform running={false} />
                  <div className="qbox">add a task to finish my DBMS revision…</div>
                </div>
                <div className="vf">
                  <button className="mbtn" aria-label="Microphone (preview only)" tabIndex={-1}>
                    <Icon name="mic" size={18} />
                  </button>
                  <span className="ibt" aria-hidden="true">
                    <Icon name="kbd" />
                  </span>
                </div>
              </div>
              <div className="tk" style={{ marginTop: 10 }}>
                <span className="ck on" aria-hidden="true">
                  <Icon name="check" />
                </span>
                <span className="tt">Finish DBMS revision</span>
                <span className="chip pr-h">
                  <Icon name="flag" />
                  High
                </span>
                <span className="chip">
                  <Icon name="cal" />
                  Today 6:00 PM
                </span>
                <span className="chip">
                  <Icon name="mic" />
                </span>
              </div>
            </div>
            <div className="pv-side">
              <div className="pv-cap">Components</div>
              <div className="row" style={{ gap: 8 }}>
                <span className="btn pri sm">Primary</span>
                <span className="btn sec sm">Secondary</span>
                <span className="btn dst sm">Delete</span>
              </div>
              <div className="tst" style={{ width: "100%" }}>
                <Icon name="pause" className="wn" />
                <div>
                  <div className="tt">Listening paused</div>
                  <div className="ts">Resume from the tray.</div>
                </div>
              </div>
              <div className="row" style={{ gap: 10 }}>
                <span className="tg on" aria-hidden="true" />
                <span style={{ fontSize: 12 }}>Spoken replies</span>
              </div>
              <select className="sel" style={{ width: "100%" }} aria-label="Microphone (preview)" disabled>
                <option>System default</option>
              </select>
              <input className="inp" placeholder="Type a command…" aria-label="Command (preview)" disabled />
              <div className="pv-cap">Activity</div>
              <div className="arh" style={{ border: "1px solid var(--bd)", borderRadius: 6 }}>
                <span className="tm">09:41</span>
                <span className="q">“Add finish my DBMS…”</span>
                <span className="bdg ok">
                  <Icon name="check" />
                  Success
                </span>
              </div>
              <div className="pv-cap">Dialog</div>
              <div className="dlg" style={{ width: "100%", padding: 14 }}>
                <h3 style={{ fontSize: 14 }}>Delete this task?</h3>
                <p style={{ fontSize: 12 }}>This can’t be undone.</p>
                <div className="dacts">
                  <span className="btn sec sm">Cancel</span>
                  <span className="btn dst sm">Delete</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="row" style={{ gap: 8, marginTop: 12, flexWrap: "wrap" }}>
        <Button variant="pri" disabled={busy} onClick={() => void apply(shownTheme)}>
          Apply Theme
        </Button>
        <Button
          variant="sec"
          disabled={!previewing}
          title={previewing ? "Discard the preview" : "Nothing is being previewed"}
          onClick={() => {
            cancelPreview();
            toastInfo("Preview discarded", `Back to ${themeName(applied.theme)}.`);
          }}
        >
          Cancel
        </Button>
        <Button
          variant="gst"
          onClick={() => {
            resetCustomization();
            toastInfo("Colors reset", `Back to the ${themeName(shownTheme)} defaults.`);
          }}
        >
          Reset
        </Button>
        <span className="grow" />
        <span className="dim" style={{ fontSize: 11.5 }}>
          {previewing
            ? "Changes stay in preview until you hit Apply"
            : "This theme is applied — customize freely below"}
        </span>
      </div>

      <div className="bnr wn" style={{ marginTop: 14 }}>
        <Icon name="info" />
        <span>
          <b>System Default</b> follows the Windows app theme — currently reading{" "}
          <b>{systemIsDark ? "dark" : "light"}</b>. A manually chosen theme stays fixed when Windows
          changes. Only Nova’s own surfaces follow it; the rest of Windows is outside Nova’s control.
        </span>
      </div>

      <div className="row" style={{ justifyContent: "space-between", margin: "18px 0 8px" }}>
        <div className="cap">
          Customize theme <span className="dim" style={{ letterSpacing: 0 }}>· applies to {themeName(shownTheme)}</span>
        </div>
        <button className="more" onClick={() => setCustomizeOpen((open) => !open)}>
          {customizeOpen ? "Hide" : "Show"}
        </button>
      </div>

      {customizeOpen ? (
        <Card style={{ margin: 0 }}>
          <div className="cbd" style={{ paddingTop: 14 }}>
            <div className="row" style={{ justifyContent: "space-between" }}>
              <div className="stt" style={{ fontSize: 12.5 }}>
                Accent color
              </div>
              <span className="dim" style={{ fontSize: 11.5 }}>
                Contrast is checked per swatch — text stays readable
              </span>
            </div>
            <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
              {ACCENTS.map((accent) => (
                <button
                  key={accent}
                  className="ibt"
                  aria-label={`Accent ${accent}`}
                  title={accent}
                  style={{
                    width: 26,
                    height: 26,
                    borderRadius: "50%",
                    background: accent,
                    border: `2px solid ${active.accent === accent ? "var(--fg)" : "transparent"}`,
                    boxShadow: `0 0 8px ${rgba(accent, 0.5)}`,
                  }}
                  onClick={() => setAccent(active.accent === accent ? null : accent)}
                />
              ))}
              <label
                className="ibt"
                aria-label="Custom accent color"
                title="Custom accent color"
                style={{
                  width: 26,
                  height: 26,
                  borderRadius: "50%",
                  position: "relative",
                  border: "1px dashed var(--bd2)",
                  overflow: "hidden",
                  padding: 0,
                }}
              >
                <input
                  type="color"
                  value={active.accent ?? "#3fe0ff"}
                  style={{ position: "absolute", inset: -6, opacity: 0, cursor: "pointer" }}
                  onChange={(event) => setAccent(event.target.value)}
                />
              </label>
              {active.accent ? (
                <button className="more" onClick={() => setAccent(null)}>
                  Use the theme accent
                </button>
              ) : null}
            </div>

            <div className="row" style={{ justifyContent: "space-between", marginTop: 6 }}>
              <div className="stt" style={{ fontSize: 12.5 }}>
                Interface density
              </div>
              <Segmented
                label="Interface density"
                value={active.density}
                onChange={(value) => setDensity(value)}
                options={DENSITIES}
              />
            </div>

            <div style={{ marginTop: 8 }}>
              <div className="cap" style={{ marginBottom: 2 }}>
                Background effects{" "}
                <span className="dim" style={{ letterSpacing: 0, textTransform: "none" }}>
                  · apply immediately
                </span>
              </div>
              <EffectRow
                label="Animated particles"
                note="Slow luminous drift across the environment."
                checked={active.effects.particles}
                onChange={(value) => setEffect("particles", value)}
              />
              <EffectRow
                label="Network lines & data streams"
                note="Neural connections, perspective grid and light pulses."
                checked={active.effects.network}
                onChange={(value) => setEffect("network", value)}
              />
              <EffectRow
                label="Background parallax"
                note="Depth-scaled motion between layers."
                checked={active.effects.parallax}
                onChange={(value) => setEffect("parallax", value)}
              />
              <EffectRow
                label="Voice-reactive effects"
                note="The environment energizes while Nova is listening."
                checked={active.effects.voice}
                onChange={(value) => setEffect("voice", value)}
              />
            </div>

            <div className="row" style={{ justifyContent: "space-between", marginTop: 12 }}>
              <div className="stt" style={{ fontSize: 12.5 }}>
                Glow intensity
              </div>
              <span className="row" style={{ gap: 10 }}>
                <input
                  className="rng"
                  type="range"
                  min={0}
                  max={150}
                  value={active.effects.glow}
                  aria-label="Glow intensity"
                  onChange={(event) => setEffect("glow", Number(event.target.value))}
                />
                <span className="dim" style={{ fontFamily: "var(--fm)", fontSize: 11, width: 38, textAlign: "right" }}>
                  {active.effects.glow}%
                </span>
              </span>
            </div>

            <div className="row" style={{ justifyContent: "space-between", marginTop: 8 }}>
              <div className="stt" style={{ fontSize: 12.5 }}>
                Panel transparency{" "}
                {!blurSupported ? <span className="stag">Unavailable</span> : null}
              </div>
              <span className="row" style={{ gap: 10 }}>
                <input
                  className="rng"
                  type="range"
                  min={60}
                  max={100}
                  value={active.effects.transparency}
                  aria-label="Panel transparency"
                  disabled={!blurSupported}
                  title={
                    blurSupported
                      ? "How opaque translucent panels are"
                      : "This system can’t render translucent panels"
                  }
                  onChange={(event) => setEffect("transparency", Number(event.target.value))}
                />
                <span className="dim" style={{ fontFamily: "var(--fm)", fontSize: 11, width: 38, textAlign: "right" }}>
                  {blurSupported ? `${active.effects.transparency}%` : "—"}
                </span>
              </span>
            </div>
            {!blurSupported ? (
              <div className="bnr wn" style={{ marginTop: 8, marginBottom: 0 }}>
                <Icon name="info" />
                <span>
                  Panel blur isn’t available in this WebView, so Nova is drawing solid surfaces
                  instead. Transparency has no effect here; everything else still applies.
                </span>
              </div>
            ) : null}

            <div className="row" style={{ justifyContent: "space-between", marginTop: 10 }}>
              <div className="stt" style={{ fontSize: 12.5 }}>
                Animation intensity
              </div>
              <Segmented
                label="Animation intensity"
                value={active.effects.intensity}
                onChange={(value) => setIntensity(value)}
                options={INTENSITIES}
              />
            </div>

            <div className="row" style={{ marginTop: 14, justifyContent: "space-between" }}>
              <span className="dim" style={{ fontSize: 11.5 }}>
                Essential interaction feedback stays on at every intensity. The Windows
                reduced-motion setting wins over all of this.
              </span>
              <Button
                variant="dst"
                small
                onClick={() =>
                  confirmAction({
                    title: "Reset appearance to defaults?",
                    icon: "alert",
                    destructive: true,
                    confirmLabel: "Reset appearance",
                    body: (
                      <p>
                        Theme, accent, density and background effects return to the Nova Neon
                        defaults. This can’t be undone.
                      </p>
                    ),
                    onConfirm: async () => {
                      await resetAll();
                      toastSuccess("Appearance reset", "Nova Neon restored with default effects.");
                    },
                  })
                }
              >
                Reset appearance to defaults
              </Button>
            </div>
          </div>
        </Card>
      ) : null}
    </>
  );
}

function EffectRow({
  label,
  note,
  checked,
  onChange,
}: {
  label: string;
  note: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <div className="row" style={{ padding: "9px 0", borderBottom: "1px solid var(--bd)", alignItems: "flex-start" }}>
      <div className="st" style={{ flex: 1 }}>
        <div className="stt" style={{ fontSize: 12.5 }}>
          {label}
        </div>
        <div className="std" style={{ fontSize: 11.5 }}>
          {note}
        </div>
      </div>
      <Toggle label={label} checked={checked} onChange={onChange} />
    </div>
  );
}

/** Miniature of the real interface — every card shows the actual theme tokens. */
function MiniPreview() {
  return (
    <div className="mini">
      <span className="m-rail">
        <i />
        <i />
        <i />
      </span>
      <span className="m-top" />
      <span className="m-card">
        <b />
        <i />
        <s />
      </span>
      <span className="m-emb">
        <b />
      </span>
      <span className="m-wv">
        <i />
        <i />
        <i />
        <i />
      </span>
      <span className="m-btn" />
    </div>
  );
}
