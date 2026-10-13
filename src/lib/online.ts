// Network reachability, used for the designed "offline" states.
//
// `navigator.onLine` is a *coarse* signal (it only reflects whether Windows
// thinks it has a connection), so the UI copy never promises more than that:
// Nova's own features are local, and only cloud providers are affected.

import { useEffect, useState } from "react";

export function useOnline(): boolean {
  const [online, setOnline] = useState(() =>
    typeof navigator === "undefined" ? true : navigator.onLine,
  );

  useEffect(() => {
    const goOnline = () => setOnline(true);
    const goOffline = () => setOnline(false);
    window.addEventListener("online", goOnline);
    window.addEventListener("offline", goOffline);
    return () => {
      window.removeEventListener("online", goOnline);
      window.removeEventListener("offline", goOffline);
    };
  }, []);

  return online;
}

/** Shared wording so the three list screens describe offline identically. */
export const OFFLINE_COPY =
  "You’re offline — cloud providers can’t be reached. Your tasks and history live on this PC and keep working.";
