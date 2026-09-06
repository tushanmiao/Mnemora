import { useSyncExternalStore } from "react";
import { DEFAULT_NOTE_EDITOR_SETTINGS, type NoteEditorSettings } from "../../../types/appSettings";
let preferences = DEFAULT_NOTE_EDITOR_SETTINGS;
const listeners = new Set<() => void>();
export function publishNoteEditorPreferences(next: NoteEditorSettings = DEFAULT_NOTE_EDITOR_SETTINGS) {
  if (JSON.stringify(preferences) === JSON.stringify(next)) return;
  preferences = next; listeners.forEach((listener) => listener());
}
export function getNoteEditorPreferences() { return preferences; }
export function useNoteEditorPreferences() {
  return useSyncExternalStore((listener) => { listeners.add(listener); return () => { listeners.delete(listener); }; }, getNoteEditorPreferences);
}
