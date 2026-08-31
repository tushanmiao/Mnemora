import { invoke, isTauri } from "@tauri-apps/api/core";
import type { PromptTemplate, PromptTemplateInput } from "../../../types/prompt";

export function listPromptTemplates() {
  if (!isTauri()) return Promise.resolve<PromptTemplate[]>([]);
  return invoke<PromptTemplate[]>("prompt_templates_list");
}

export function upsertPromptTemplate(input: PromptTemplateInput) {
  if (!isTauri()) {
    const now = Date.now();
    return Promise.resolve<PromptTemplate>({
      id: input.id ?? crypto.randomUUID(),
      title: input.title.trim(),
      content: input.content.trim(),
      createdAt: now,
      updatedAt: now,
    });
  }
  return invoke<PromptTemplate>("prompt_templates_upsert", { input });
}

export function deletePromptTemplate(promptId: string) {
  if (!isTauri()) return Promise.resolve(true);
  return invoke<boolean>("prompt_templates_delete", { promptId });
}
