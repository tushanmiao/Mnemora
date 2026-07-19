/** 用于组织相关对话、文献和工作上下文的项目。 */
export interface ChatProject {
  id: string;
  name: string;
  description: string;
  color: string | null;
  systemPrompt: string;
  createdAt: number;
  updatedAt: number;
}

/** 只用于整理对话的轻量集合，不向模型注入指令。 */
export interface ConversationCollection {
  id: string;
  name: string;
  color: string | null;
  createdAt: number;
  updatedAt: number;
}
