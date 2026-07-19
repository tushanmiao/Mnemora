import type { AiPermissionMode } from "./chat";

/** 可在多个对话中复用的助手配置。 */
export interface AssistantProfile {
  id: string;
  name: string;
  description: string;
  systemPrompt: string;
  modelId: string | null;
  permissionMode: AiPermissionMode;
  createdAt: number;
  updatedAt: number;
}
