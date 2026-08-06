export type WorkspaceLifecycleState = "active" | "background" | "disposed";

const BACKGROUND_DELAY_MS = 60_000;
const listeners = new Set<(state: WorkspaceLifecycleState) => void>();
let currentState: WorkspaceLifecycleState = "active";
let backgroundTimer: number | null = null;
let initialized = false;

export function initializeWorkspaceLifecycle() {
  if (initialized || typeof document === "undefined") return () => undefined;
  initialized = true;

  const updateVisibility = () => {
    if (document.visibilityState === "visible") {
      clearBackgroundTimer();
      publish("active");
      return;
    }
    clearBackgroundTimer();
    backgroundTimer = window.setTimeout(() => {
      backgroundTimer = null;
      if (document.visibilityState !== "visible") publish("background");
    }, BACKGROUND_DELAY_MS);
  };
  const dispose = () => {
    clearBackgroundTimer();
    publish("disposed");
  };

  document.addEventListener("visibilitychange", updateVisibility);
  window.addEventListener("pagehide", dispose);
  updateVisibility();
  return () => {
    clearBackgroundTimer();
    document.removeEventListener("visibilitychange", updateVisibility);
    window.removeEventListener("pagehide", dispose);
    initialized = false;
  };
}

export function subscribeWorkspaceLifecycle(listener: (state: WorkspaceLifecycleState) => void) {
  listeners.add(listener);
  listener(currentState);
  return () => {
    listeners.delete(listener);
  };
}

export function getWorkspaceLifecycleState() {
  return currentState;
}

function clearBackgroundTimer() {
  if (backgroundTimer === null) return;
  window.clearTimeout(backgroundTimer);
  backgroundTimer = null;
}

function publish(state: WorkspaceLifecycleState) {
  if (currentState === state) return;
  currentState = state;
  for (const listener of listeners) listener(state);
}
