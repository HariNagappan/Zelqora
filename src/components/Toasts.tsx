import { useEffect } from "react";

import { Icon } from "./Icon";
import { useToastStore, type Toast } from "../stores/useToasts";

const ICONS: Record<Toast["kind"], { name: "check" | "pause" | "alert" | "info"; cls: string }> = {
  ok: { name: "check", cls: "ok" },
  wn: { name: "pause", cls: "wn" },
  er: { name: "alert", cls: "er" },
  info: { name: "info", cls: "" },
};

const LIFETIME = 5200;

function ToastRow({ toast }: { toast: Toast }) {
  const dismiss = useToastStore((state) => state.dismiss);
  const icon = ICONS[toast.kind];

  useEffect(() => {
    const timer = window.setTimeout(() => dismiss(toast.id), LIFETIME);
    return () => window.clearTimeout(timer);
  }, [dismiss, toast.id]);

  return (
    <div className="tst" role={toast.kind === "er" || toast.kind === "wn" ? "alert" : "status"}>
      <Icon name={icon.name} className={icon.cls} />
      <div className="grow">
        <div className="tt">{toast.title}</div>
        {toast.detail ? <div className="ts">{toast.detail}</div> : null}
        {toast.action ? (
          <button
            className="more"
            style={{ marginTop: 6, textDecoration: "underline" }}
            onClick={() => {
              dismiss(toast.id);
              toast.action?.run();
            }}
          >
            {toast.action.label}
          </button>
        ) : null}
      </div>
      <button
        className="ibt"
        style={{ width: 24, height: 24 }}
        aria-label="Dismiss notification"
        title="Dismiss"
        onClick={() => dismiss(toast.id)}
      >
        <Icon name="x" size={14} />
      </button>
    </div>
  );
}

/** Toast host — one per window, `aria-live` so screen readers announce it. */
export default function Toasts() {
  const toasts = useToastStore((state) => state.toasts);
  return (
    <div className="tstw" aria-live="polite">
      {toasts.map((toast) => (
        <ToastRow key={toast.id} toast={toast} />
      ))}
    </div>
  );
}
