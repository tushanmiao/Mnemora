import { invoke, isTauri } from "@tauri-apps/api/core";
import type { AppSettings } from "../../types/appSettings";

export function setPetEnabled(enabled: boolean) {
  if (!isTauri()) return Promise.resolve<AppSettings | null>(null);
  return invoke<AppSettings>("pet_set_enabled", { enabled });
}

export function updatePetPosition(x: number, y: number) {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("pet_update_position", { x, y });
}

export function openMainFromPet() {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("pet_open_main");
}
