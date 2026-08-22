export type PetState = "idle" | "thinking" | "tooling" | "waiting" | "success" | "error";

export interface PetStatePayload {
  state: PetState;
  label: string;
  detail: string;
  updatedAt: number;
}

export interface PetDescriptor {
  id: string;
  displayName: string;
  description: string;
  kind: string;
  source: "builtin" | "local";
  selected: boolean;
  spritesheetUrl: string | null;
  atlasWidth: number | null;
  atlasHeight: number | null;
  columns: number | null;
  rows: number | null;
  compatible: boolean;
  compatibilityMessage: string | null;
}

export interface CodexPetImportResult {
  found: number;
  imported: number;
  selectedPetId: string | null;
  failures: string[];
  pets: PetDescriptor[];
}
