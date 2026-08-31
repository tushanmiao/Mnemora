/** 可以快速插入用户输入框的普通提示词模板。 */
export interface PromptTemplate {
  id: string;
  title: string;
  content: string;
  createdAt: number;
  updatedAt: number;
}

export interface PromptTemplateInput {
  id?: string;
  title: string;
  content: string;
}

/** 最终 System Prompt 中某一段内容的来源。 */
export type SystemPromptSource =
  | "builtin"
  | "global"
  | "assistant"
  | "project"
  | "conversation"
  | "memory"
  | "documentContext"
  | "tools";

/** 一段带来源信息、可调试和审计的 System Prompt。 */
export interface SystemPromptSection {
  id: string;
  source: SystemPromptSource;
  title: string;
  content: string;
}
