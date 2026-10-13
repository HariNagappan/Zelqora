// Date/time formatting shared by the Tasks, Activity and Home screens.
// Due dates are stored in UTC and always displayed in the user's local time.

const MIDNIGHT = { hour: 0, minute: 0, second: 0, millisecond: 0 };

function startOfToday(): Date {
  const now = new Date();
  now.setHours(0, 0, 0, 0);
  return now;
}

export function isOverdue(dueAt: string | null, completed: boolean): boolean {
  if (!dueAt || completed) {
    return false;
  }
  const due = new Date(dueAt);
  return !Number.isNaN(due.getTime()) && due.getTime() < Date.now();
}

/** "Today 6:00 PM" · "Tomorrow" · "Fri, Oct 16" · "Overdue since Oct 9". */
export function formatDue(dueAt: string | null, completed = false): string | null {
  if (!dueAt) {
    return null;
  }
  const due = new Date(dueAt);
  if (Number.isNaN(due.getTime())) {
    return null;
  }
  const today = startOfToday();
  const dueDay = new Date(due);
  dueDay.setHours(0, 0, 0, 0);
  const dayDiff = Math.round((dueDay.getTime() - today.getTime()) / 86_400_000);
  const hasTime = due.getHours() !== MIDNIGHT.hour || due.getMinutes() !== MIDNIGHT.minute;
  const time = due.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

  if (completed) {
    return `Done · ${due.toLocaleDateString([], { month: "short", day: "numeric" })}`;
  }
  if (dayDiff === 0) {
    return hasTime ? `Today ${time}` : "Today";
  }
  if (dayDiff === 1) {
    return hasTime ? `Tomorrow ${time}` : "Tomorrow";
  }
  if (dayDiff === -1) {
    return hasTime ? `Overdue · yesterday ${time}` : "Overdue · yesterday";
  }
  if (dayDiff < 0) {
    const label = due.toLocaleDateString([], { month: "short", day: "numeric" });
    return `Overdue · ${label}`;
  }
  const label = due.toLocaleDateString([], { weekday: "short", month: "short", day: "numeric" });
  return hasTime ? `${label} ${time}` : label;
}

/** Local `YYYY-MM-DDTHH:mm` value for `<input type="datetime-local">`. */
export function toDateTimeLocal(value: string | null): string {
  if (!value) {
    return "";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return "";
  }
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** Convert an `<input type="datetime-local">` value into a UTC ISO string. */
export function fromDateTimeLocal(value: string): string | null {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  return date.toISOString();
}

/** Compact clock label for the Activity list ("09:41", "Yest. 22:31"). */
export function formatActivityTime(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    return timestamp;
  }
  const today = startOfToday();
  const day = new Date(date);
  day.setHours(0, 0, 0, 0);
  const diff = Math.round((today.getTime() - day.getTime()) / 86_400_000);
  const time = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  if (diff === 0) {
    return time;
  }
  if (diff === 1) {
    return `Yest. ${time}`;
  }
  return date.toLocaleDateString([], { month: "short", day: "numeric" });
}

export function formatRelative(timestamp: string): string {
  const then = new Date(timestamp).getTime();
  if (Number.isNaN(then)) {
    return timestamp;
  }
  const seconds = Math.max(0, Math.round((Date.now() - then) / 1000));
  if (seconds < 60) {
    return "just now";
  }
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) {
    return `${minutes} min ago`;
  }
  const hours = Math.round(minutes / 60);
  if (hours < 24) {
    return `${hours} h ago`;
  }
  const days = Math.round(hours / 24);
  if (days === 1) {
    return "yesterday";
  }
  if (days < 7) {
    return `${days} days ago`;
  }
  return new Date(timestamp).toLocaleDateString();
}

export function greeting(now = new Date()): string {
  const hour = now.getHours();
  if (hour < 12) {
    return "Good morning";
  }
  if (hour < 18) {
    return "Good afternoon";
  }
  return "Good evening";
}

export function priorityLabel(priority: string): string {
  return priority === "high" ? "High" : priority === "low" ? "Low" : "Medium";
}

export function titleCase(value: string): string {
  return value.length === 0 ? value : value[0].toUpperCase() + value.slice(1);
}
