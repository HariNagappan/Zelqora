// Icon set — the fine-line geometric family from the design canvas.
// Icons are real SVG symbols (never emoji), rendered through one shared sprite.

export type IconName =
  | "nova"
  | "home"
  | "tasks"
  | "clock"
  | "set"
  | "mic"
  | "micoff"
  | "search"
  | "plus"
  | "x"
  | "check"
  | "ckd"
  | "ckr"
  | "trash"
  | "pen"
  | "refresh"
  | "alert"
  | "info"
  | "folder"
  | "cal"
  | "flag"
  | "shield"
  | "kbd"
  | "exp"
  | "power"
  | "pause"
  | "play"
  | "sun"
  | "moon"
  | "spark"
  | "cpu"
  | "min"
  | "max";

export function Icon({
  name,
  size = 16,
  className = "",
  title,
}: {
  name: IconName;
  size?: number;
  className?: string;
  title?: string;
}) {
  const classes = className ? `ic ${className}` : "ic";
  return (
    <svg className={classes} width={size} height={size} aria-hidden={title ? undefined : true} role={title ? "img" : undefined}>
      {title ? <title>{title}</title> : null}
      <use href={`#i-${name}`} />
    </svg>
  );
}

/** Rendered once at the root of every window. */
export function IconSprite() {
  return (
    <svg style={{ display: "none" }} aria-hidden="true">
      <defs>
        <symbol id="i-nova" viewBox="0 0 24 24">
          <circle cx="11" cy="12" r="3.8" />
          <path d="M16.1 7.4a6.9 6.9 0 0 1 0 9.2M19.5 4.9a10.9 10.9 0 0 1 0 14.2" />
          <circle cx="11" cy="12" r="1.3" fill="currentColor" stroke="none" />
        </symbol>
        <symbol id="i-home" viewBox="0 0 16 16">
          <path d="M2.5 7.2 8 2.6l5.5 4.6V13a.7.7 0 0 1-.7.7H3.2a.7.7 0 0 1-.7-.7Z" />
          <path d="M5.7 13.7V9.3h4.6v4.4" />
        </symbol>
        <symbol id="i-tasks" viewBox="0 0 16 16">
          <path d="m2.5 4.3 1.4 1.4L6.1 3.4M2.5 9.6 3.9 11l2.2-2.3M9 4.7h4.5M9 10h4.5" />
        </symbol>
        <symbol id="i-clock" viewBox="0 0 16 16">
          <circle cx="8" cy="8" r="5.6" />
          <path d="M8 5.1V8l2 1.4" />
        </symbol>
        <symbol id="i-set" viewBox="0 0 16 16">
          <path d="M2.5 4.7h6.3M12.2 4.7h1.3M2.5 8h1.3M7.2 8h6.3M2.5 11.3h4.3M10.2 11.3h3.3" />
          <circle cx="10.5" cy="4.7" r="1.7" />
          <circle cx="5.5" cy="8" r="1.7" />
          <circle cx="8.5" cy="11.3" r="1.7" />
        </symbol>
        <symbol id="i-mic" viewBox="0 0 16 16">
          <rect x="6" y="1.8" width="4" height="7.4" rx="2" />
          <path d="M3.6 7.4a4.4 4.4 0 0 0 8.8 0M8 11.8v2.4M5.8 14.2h4.4" />
        </symbol>
        <symbol id="i-micoff" viewBox="0 0 16 16">
          <rect x="6" y="1.8" width="4" height="7.4" rx="2" />
          <path d="M3.6 7.4a4.4 4.4 0 0 0 8.8 0M8 11.8v2.4M5.8 14.2h4.4M2.5 2.5l11 11" />
        </symbol>
        <symbol id="i-search" viewBox="0 0 16 16">
          <circle cx="6.9" cy="6.9" r="4.4" />
          <path d="m10.2 10.2 3.3 3.3" />
        </symbol>
        <symbol id="i-plus" viewBox="0 0 16 16">
          <path d="M8 3v10M3 8h10" />
        </symbol>
        <symbol id="i-x" viewBox="0 0 16 16">
          <path d="m4 4 8 8M12 4l-8 8" />
        </symbol>
        <symbol id="i-check" viewBox="0 0 16 16">
          <path d="m3.2 8.4 3.3 3.3 6.3-6.8" />
        </symbol>
        <symbol id="i-ckd" viewBox="0 0 16 16">
          <path d="m4.5 6.2 3.5 3.6 3.5-3.6" />
        </symbol>
        <symbol id="i-ckr" viewBox="0 0 16 16">
          <path d="m6.2 4 3.6 4-3.6 4" />
        </symbol>
        <symbol id="i-trash" viewBox="0 0 16 16">
          <path d="M3 4.6h10M6.4 4.6V3.3c0-.4.4-.8.8-.8h1.6c.4 0 .8.4.8.8v1.3M4.6 4.6l.5 8.2c0 .4.4.7.8.7h4.2c.4 0 .8-.3.8-.7l.5-8.2M6.7 7.2v3.9M9.3 7.2v3.9" />
        </symbol>
        <symbol id="i-pen" viewBox="0 0 16 16">
          <path d="M11.1 2.7 13.3 4.9 5.1 13.1l-2.9.7.7-2.9ZM9.6 4.2l2.2 2.2" />
        </symbol>
        <symbol id="i-refresh" viewBox="0 0 16 16">
          <path d="M13.6 6.7A5.9 5.9 0 1 0 14 9.3" />
          <path d="M13.9 2.9v3.8h-3.8" />
        </symbol>
        <symbol id="i-alert" viewBox="0 0 16 16">
          <path d="M8 2.6 14.5 13.4H1.5ZM8 6.6v3M8 11.4v.2" />
        </symbol>
        <symbol id="i-info" viewBox="0 0 16 16">
          <circle cx="8" cy="8" r="5.8" />
          <path d="M8 7.4v3.4M8 5v.2" />
        </symbol>
        <symbol id="i-folder" viewBox="0 0 16 16">
          <path d="M2.2 12.6V4.4c0-.5.4-.9.9-.9h3.2l1.5 1.9h5.1c.5 0 .9.4.9.9v6.3c0 .5-.4.9-.9.9H3.1a.9.9 0 0 1-.9-.9Z" />
        </symbol>
        <symbol id="i-cal" viewBox="0 0 16 16">
          <rect x="2.4" y="3.4" width="11.2" height="10.4" rx="1.4" />
          <path d="M5.2 2.2v2.4M10.8 2.2v2.4M2.4 6.8h11.2" />
        </symbol>
        <symbol id="i-flag" viewBox="0 0 16 16">
          <path d="M4 13.8V2.6M4 3.4h8.2l-2.1 2.7 2.1 2.7H4" />
        </symbol>
        <symbol id="i-shield" viewBox="0 0 16 16">
          <path d="M8 1.9 13 3.9v4.3c0 3-2 5-5 6.1-3-1.1-5-3.1-5-6.1V3.9Z" />
          <path d="m6.1 8 1.4 1.4 2.6-2.7" />
        </symbol>
        <symbol id="i-kbd" viewBox="0 0 16 16">
          <rect x="1.4" y="3.8" width="13.2" height="8.4" rx="1.4" />
          <path d="M4 6.4h.1M6.5 6.4h.1M9 6.4h.1M11.5 6.4h.1M4 9.6h8" />
        </symbol>
        <symbol id="i-exp" viewBox="0 0 16 16">
          <path d="M9.3 2.5h4.2v4.2M2.5 9.3v4.2h4.2M13.5 2.5 9.6 6.4M2.5 13.5l3.9-3.9" />
        </symbol>
        <symbol id="i-power" viewBox="0 0 16 16">
          <path d="M8 2.3v5.2M4.4 4.3a5.1 5.1 0 1 0 7.2 0" />
        </symbol>
        <symbol id="i-pause" viewBox="0 0 16 16">
          <path d="M6 3.4v9.2M10 3.4v9.2" />
        </symbol>
        <symbol id="i-play" viewBox="0 0 16 16">
          <path d="M5.4 3.2v9.6L13 8Z" />
        </symbol>
        <symbol id="i-sun" viewBox="0 0 16 16">
          <circle cx="8" cy="8" r="3.1" />
          <path d="M8 1.5v1.6M8 12.9v1.6M1.5 8h1.6M12.9 8h1.6M3.4 3.4l1.1 1.1M11.5 11.5l1.1 1.1M12.6 3.4l-1.1 1.1M4.5 11.5l-1.1 1.1" />
        </symbol>
        <symbol id="i-moon" viewBox="0 0 16 16">
          <path d="M13.4 9.7A5.9 5.9 0 1 1 6.3 2.6a4.7 4.7 0 0 0 7.1 7.1Z" />
        </symbol>
        <symbol id="i-spark" viewBox="0 0 16 16">
          <path d="M8 2.2 9.2 6.8 13.8 8 9.2 9.2 8 13.8 6.8 9.2 2.2 8 6.8 6.8Z" />
        </symbol>
        <symbol id="i-cpu" viewBox="0 0 16 16">
          <rect x="4.4" y="4.4" width="7.2" height="7.2" rx="1.2" />
          <path d="M8 1.8v2.6M8 11.6v2.6M1.8 8h2.6M11.6 8h2.6" />
        </symbol>
        <symbol id="i-min" viewBox="0 0 16 16">
          <path d="M3.5 8h9" />
        </symbol>
        <symbol id="i-max" viewBox="0 0 16 16">
          <rect x="3.4" y="3.4" width="9.2" height="9.2" rx="1" />
        </symbol>
      </defs>
    </svg>
  );
}
