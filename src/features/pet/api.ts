import { convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { AppSettings } from "../../types/appSettings";
import type { CodexPetImportResult, PetDescriptor } from "./types";

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

export async function listPets() {
  if (!isTauri()) return [] as PetDescriptor[];
  const pets = await invoke<PetDescriptor[]>("pet_list");
  return pets.map(resolvePetAssetUrl);
}

export async function importPetPackage() {
  if (!isTauri()) return null;
  const path = await open({
    title: "选择宠物包目录",
    multiple: false,
    directory: true,
  });
  if (typeof path !== "string") return null;
  const pets = await invoke<PetDescriptor[]>("pet_import", { path });
  return pets.map(resolvePetAssetUrl);
}

export async function installPetArchive() {
  if (!isTauri()) return null;
  const path = await open({
    title: "安装宠物 ZIP",
    multiple: false,
    directory: false,
    filters: [{ name: "Mnemora / hatch-pet 宠物包", extensions: ["zip"] }],
  });
  if (typeof path !== "string") return null;
  const pets = await invoke<PetDescriptor[]>("pet_import_archive", { path });
  return pets.map(resolvePetAssetUrl);
}

export async function importCodexPets() {
  if (!isTauri()) return null;
  const result = await invoke<CodexPetImportResult>("pet_import_codex");
  return { ...result, pets: result.pets.map(resolvePetAssetUrl) };
}

export async function deletePet(petId: string) {
  if (!isTauri()) return [] as PetDescriptor[];
  const pets = await invoke<PetDescriptor[]>("pet_delete", { petId });
  return pets.map(resolvePetAssetUrl);
}

export function openPetDirectory() {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("pet_open_directory");
}

function resolvePetAssetUrl(pet: PetDescriptor): PetDescriptor {
  return pet.spritesheetUrl
    ? { ...pet, spritesheetUrl: convertFileSrc(pet.spritesheetUrl) }
    : pet;
}
