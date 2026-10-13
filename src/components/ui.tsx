// Shared UI primitives — the component library from the design canvas.
// Every component consumes theme variables only; no colours are hardcoded.

import type { ButtonHTMLAttributes, ChangeEvent, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from "react";
import { useEffect, useRef } from "react";

import { Icon, type IconName } from "./Icon";
import { STATIC_WAVE, getLevel, onLevel } from "../lib/level";

type ButtonVariant = "pri" | "sec" | "gst" | "dst";

export function Button({
  variant = "sec",
  small = false,
  icon,
  iconRight,
  children,
  className = "",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  small?: boolean;
  icon?: IconName;
  iconRight?: IconName;
}) {
  const classes = ["btn", variant, small ? "sm" : "", className].filter(Boolean).join(" ");
  return (
    <button type="button" className={classes} {...rest}>
      {icon ? <Icon name={icon} size={small ? 13 : 14} /> : null}
      {children}
      {iconRight ? <Icon name={iconRight} size={12} /> : null}
    </button>
  );
}

export function IconButton({
  icon,
  label,
  size = 16,
  className = "",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { icon: IconName; label: string; size?: number }) {
  return (
    <button type="button" className={`ibt ${className}`} aria-label={label} title={label} {...rest}>
      <Icon name={icon} size={size} />
    </button>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={label}
      className={`tg${checked ? " on" : ""}${disabled ? " dis" : ""}`}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    />
  );
}

export function Checkbox({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-label={label}
      title={label}
      className={`ck${checked ? " on" : ""}`}
      onClick={() => onChange(!checked)}
    >
      {checked ? <Icon name="check" /> : null}
    </button>
  );
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
}: {
  options: { id: T; label: string }[];
  value: T;
  onChange: (next: T) => void;
  label?: string;
}) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.id}
          type="button"
          className={value === option.id ? "on" : ""}
          aria-pressed={value === option.id}
          onClick={() => onChange(option.id)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Badge({
  kind = "mut",
  icon,
  children,
}: {
  kind?: "ok" | "er" | "wn" | "mut";
  icon?: IconName;
  children: ReactNode;
}) {
  return (
    <span className={`bdg ${kind}`}>
      {icon ? <Icon name={icon} /> : null}
      {children}
    </span>
  );
}

export function Chip({
  icon,
  children,
  className = "",
  title,
}: {
  icon?: IconName;
  children: ReactNode;
  className?: string;
  title?: string;
}) {
  return (
    <span className={`chip ${className}`} title={title}>
      {icon ? <Icon name={icon} /> : null}
      {children}
    </span>
  );
}

export function Dot({ kind = "mut", live = false }: { kind?: "ok" | "er" | "wn" | "mut"; live?: boolean }) {
  return <span className={`dot ${kind}${live ? " live" : ""}`} aria-hidden="true" />;
}

export function Progress() {
  return (
    <div className="pgb" role="progressbar" aria-label="Working">
      <i />
    </div>
  );
}

export function Skeleton({
  width,
  height,
  className = "",
}: {
  width?: number | string;
  height?: number | string;
  className?: string;
}) {
  return <span className={`skl ${className}`} style={{ width, height, display: "block" }} />;
}

export function SkeletonTasks({ rows = 4 }: { rows?: number }) {
  return (
    <div className="tlst" aria-busy="true" aria-label="Loading tasks">
      {Array.from({ length: rows }, (_, index) => (
        <div className="tk" key={index}>
          <Skeleton width={18} height={18} />
          <Skeleton width="45%" height={14} />
          <span className="grow" />
          <Skeleton width={96} height={16} />
          <Skeleton width={64} height={16} />
        </div>
      ))}
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  detail,
  action,
}: {
  icon: IconName;
  title: string;
  detail?: string;
  action?: ReactNode;
}) {
  return (
    <div className="emp">
      <span className="eic">
        <Icon name={icon} size={20} />
      </span>
      <h2>{title}</h2>
      {detail ? <p className="mut" style={{ fontSize: 13 }}>{detail}</p> : null}
      {action}
    </div>
  );
}

export function Banner({
  kind,
  icon,
  children,
}: {
  kind: "wn" | "er" | "info";
  icon?: IconName;
  children: ReactNode;
}) {
  const fallback: IconName = kind === "wn" ? "alert" : "info";
  return (
    <div className={`bnr ${kind}`} role={kind === "er" ? "alert" : "status"}>
      <Icon name={icon ?? fallback} />
      <span className="grow">{children}</span>
    </div>
  );
}

export function Card({
  children,
  className = "",
  style,
}: {
  children: ReactNode;
  className?: string;
  style?: React.CSSProperties;
}) {
  return (
    <section className={`card ${className}`} style={style}>
      {children}
    </section>
  );
}

export function CardHead({ children, icon }: { children: ReactNode; icon?: IconName }) {
  return (
    <div className="chd">
      {icon ? <Icon name={icon} /> : null}
      {children}
    </div>
  );
}

export function Field({
  label,
  hint,
  error,
  children,
  htmlFor,
}: {
  label: string;
  hint?: string;
  error?: string | null;
  children: ReactNode;
  htmlFor?: string;
}) {
  return (
    <div className="fld">
      <label className="lbl" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {error ? (
        <div className="emsg" role="alert">
          <Icon name="alert" size={13} />
          <span>{error}</span>
        </div>
      ) : hint ? (
        <span className="hint">{hint}</span>
      ) : null}
    </div>
  );
}

export function TextInput({
  className = "",
  ...rest
}: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={`inp ${className}`} {...rest} />;
}

export function Select({
  className = "",
  children,
  ...rest
}: SelectHTMLAttributes<HTMLSelectElement> & { children: ReactNode }) {
  return (
    <select className={`sel ${className}`} {...rest}>
      {children}
    </select>
  );
}

export function SearchField({
  value,
  onChange,
  placeholder,
  label,
}: {
  value: string;
  onChange: (next: string) => void;
  placeholder: string;
  label: string;
}) {
  return (
    <div className="srch">
      <Icon name="search" />
      <input
        className="inp"
        value={value}
        placeholder={placeholder}
        aria-label={label}
        onChange={(event: ChangeEvent<HTMLInputElement>) => onChange(event.target.value)}
      />
    </div>
  );
}

/**
 * Waveform. When `running` it mirrors the live microphone level; the bars are
 * updated through refs (no React re-render per audio frame).
 */
export function Waveform({ running, bars = STATIC_WAVE }: { running: boolean; bars?: number[] }) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !running) {
      return;
    }
    let frame = 0;
    let smoothed = getLevel();
    const nodes = Array.from(container.querySelectorAll("i"));
    const tick = () => {
      const live = getLevel();
      smoothed = smoothed * 0.6 + live * 0.4;
      nodes.forEach((node, index) => {
        const base = bars[index % bars.length];
        // A quiet signal keeps a gentle idle motion; loud speech scales up.
        const height = Math.max(3, base * (0.35 + smoothed * 2.6));
        (node as HTMLElement).style.height = `${height.toFixed(1)}px`;
      });
      frame = window.requestAnimationFrame(tick);
    };
    const unsubscribe = onLevel(() => {});
    frame = window.requestAnimationFrame(tick);
    return () => {
      window.cancelAnimationFrame(frame);
      unsubscribe();
    };
  }, [running, bars]);

  return (
    <div className={running ? "wv run" : "wv"} ref={containerRef} role="img" aria-label="Microphone audio level">
      {bars.map((height, index) => (
        <i key={index} style={{ "--h": `${height}px`, animationDelay: `${index * 95}ms` } as React.CSSProperties} />
      ))}
    </div>
  );
}

/** Microphone test meter (Settings + onboarding): nine bars driven by level. */
export function LevelMeter({ running, bars = 9 }: { running: boolean; bars?: number }) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    let frame = 0;
    let smoothed = 0;
    const nodes = Array.from(container.querySelectorAll("i"));
    const tick = () => {
      const live = running ? getLevel() : 0;
      smoothed = smoothed * 0.7 + live * 0.3;
      const centre = (nodes.length - 1) / 2;
      nodes.forEach((node, index) => {
        const distance = Math.abs(index - centre) / Math.max(1, centre);
        const height = 4 + smoothed * 32 * (1 - distance * 0.55);
        (node as HTMLElement).style.height = `${Math.max(4, height).toFixed(1)}px`;
      });
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(frame);
  }, [running, bars]);

  return (
    <div className="lm" ref={containerRef} role="img" aria-label="Microphone level">
      {Array.from({ length: bars }, (_, index) => (
        <i key={index} />
      ))}
    </div>
  );
}

/** The Nova emblem used in the overlay, sidebar and brand marks. */
export function Emblem({ state = "", size = 30 }: { state?: "lis" | "pro" | "ok" | "er" | ""; size?: number }) {
  return (
    <span className={`emb${state ? ` ${state}` : ""}`} style={{ width: size, height: size }} aria-hidden="true">
      <i />
      <i />
      <i />
      <b />
    </span>
  );
}
