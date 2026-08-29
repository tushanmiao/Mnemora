import { useMemo, useSyncExternalStore } from "react";
import { speechController } from "./speechController";

export function useSpeechPlayback() {
  const snapshot = useSyncExternalStore(
    speechController.subscribe,
    speechController.getSnapshot,
    speechController.getSnapshot,
  );
  const actions = useMemo(() => ({
    speak: speechController.speak.bind(speechController),
    toggle: speechController.toggle.bind(speechController),
    pause: speechController.pause.bind(speechController),
    resume: speechController.resume.bind(speechController),
    stop: speechController.stop.bind(speechController),
  }), []);

  return {
    ...snapshot,
    ...actions,
  };
}
