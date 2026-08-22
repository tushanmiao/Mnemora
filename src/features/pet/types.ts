export type PetState = "idle" | "thinking" | "tooling" | "waiting" | "success" | "error";

export interface PetStatePayload {
  state: PetState;
  label: string;
  detail: string;
  updatedAt: number;
}
