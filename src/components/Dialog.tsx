// Dialog host: reusable confirmations, error explanations and forms.
//
// One store drives every modal so the behaviour (Esc to dismiss, backdrop
// click, focus handling, focus return) is identical everywhere.

import { useEffect, useRef, type ReactNode } from "react";
import { create } from "zustand";

import { Icon, type IconName } from "./Icon";
import { Button } from "./ui";

export interface DialogConfig {
  title: string;
  icon?: IconName;
  tone?: "er" | "wn" | "";
  body?: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
  /** Rendered instead of the default actions when provided. */
  actions?: ReactNode;
  onConfirm?: () => void | Promise<void>;
  onCancel?: () => void;
}

interface DialogStore {
  config: DialogConfig | null;
  open: (config: DialogConfig) => void;
  close: () => void;
}

export const useDialogStore = create<DialogStore>((set) => ({
  config: null,
  open: (config) => set({ config }),
  close: () => set({ config: null }),
}));

/** Ask for confirmation before a destructive or consequential action. */
export function confirmAction(config: DialogConfig): void {
  useDialogStore.getState().open(config);
}

export default function DialogHost() {
  const config = useDialogStore((state) => state.config);
  const close = useDialogStore((state) => state.close);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const previousFocus = useRef<Element | null>(null);

  useEffect(() => {
    if (config) {
      previousFocus.current = document.activeElement;
      // Focus the affirmative action so Enter/Space confirms immediately and
      // keyboard users land inside the dialog.
      const buttons = dialogRef.current?.querySelectorAll<HTMLButtonElement>(".dacts button");
      buttons?.[buttons.length - 1]?.focus();
    } else if (previousFocus.current instanceof HTMLElement) {
      previousFocus.current.focus();
      previousFocus.current = null;
    }
  }, [config]);

  useEffect(() => {
    if (!config) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        config.onCancel?.();
        close();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [close, config]);

  if (!config) {
    return null;
  }

  const tone = config.tone ?? "";
  const icon = config.icon ?? (config.destructive ? "alert" : tone === "er" ? "alert" : "info");

  return (
    <div
      className="dlgHost"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          config.onCancel?.();
          close();
        }
      }}
    >
      <div className="dlg" role="dialog" aria-modal="true" aria-label={config.title} ref={dialogRef}>
        <h3>
          <Icon name={icon} className={tone} />
          {config.title}
        </h3>
        {config.body ? <div>{config.body}</div> : null}
        <div className="dacts">
          {config.actions ?? (
            <>
              <Button
                variant="sec"
                onClick={() => {
                  config.onCancel?.();
                  close();
                }}
              >
                {config.cancelLabel ?? "Cancel"}
              </Button>
              <Button
                variant={config.destructive ? "dst" : "pri"}
                onClick={() => {
                  void config.onConfirm?.();
                  close();
                }}
              >
                {config.confirmLabel ?? "Continue"}
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
