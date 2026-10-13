// Microphone level store.
//
// Levels arrive ~20 times per second while Nova is listening or testing the
// microphone. They deliberately bypass React state: the waveform component
// subscribes and writes to the DOM directly, so the ambient animation and the
// audio meter never trigger a re-render of the interface.

type Listener = (level: number) => void;

let level = 0;
let testing = false;
const listeners = new Set<Listener>();

export function setLevel(next: number, isTesting: boolean): void {
  level = Math.min(1, Math.max(0, next));
  testing = isTesting;
  for (const listener of listeners) {
    listener(level);
  }
}

export function getLevel(): number {
  return level;
}

export function isMicrophoneTestRunning(): boolean {
  return testing;
}

export function onLevel(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Fixed bar heights used when there is no live audio (idle decoration), taken
 * from the design's eight-bar waveform.
 */
export const STATIC_WAVE = [10, 20, 26, 13, 24, 16, 9, 18];
