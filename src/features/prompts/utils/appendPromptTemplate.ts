/** 提示词是可见的用户消息片段：追加到末尾、保留一个空行、绝不自动发送。 */
export function appendPromptTemplate(draft: string, content: string) {
  const prompt = content.trim();
  if (!prompt) return draft;
  return draft.trim() ? `${draft.trimEnd()}\n\n${prompt}` : prompt;
}
