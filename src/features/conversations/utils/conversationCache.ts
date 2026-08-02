import type { Conversation } from "../../../types/conversation";

export const MAX_CACHED_CONVERSATIONS = 4;
export const MAX_CACHED_TEXT_BYTES = 8 * 1024 * 1024;

/**
 * 估算会话在前端缓存中的主要文本体积。
 * 该值只用于 LRU 驱逐，不代表浏览器实际堆内存。
 */
export function estimateConversationTextBytes(conversation: Conversation) {
  let characters = conversation.title.length
    + conversation.systemPrompt.length
    + conversation.contextSummary.length;

  for (const message of conversation.messages) {
    characters += message.content.length;
    characters += message.reasoning?.length ?? 0;
    characters += message.errorMessage?.length ?? 0;
    for (const reference of message.literatureReferences ?? []) {
      characters += reference.title.length;
      characters += reference.text.length;
    }
    for (const reference of message.noteReferences ?? []) {
      characters += reference.noteTitle.length;
      characters += reference.revisionHash.length;
      characters += reference.selectedText.length;
    }
    for (const attachment of message.attachments ?? []) {
      characters += attachment.name.length;
      characters += attachment.mimeType.length;
      characters += attachment.path.length;
      characters += attachment.previewPath?.length ?? 0;
    }
    if (message.modelSnapshot) {
      characters += message.modelSnapshot.apiModel.length;
      characters += message.modelSnapshot.displayName.length;
      characters += message.modelSnapshot.providerName.length;
    }
  }

  // JS 字符串通常使用一到两个字节表示字符；按 UTF-16 上界估算，并给对象结构留固定余量。
  return characters * 2 + conversation.messages.length * 256 + 1_024;
}

type TrimConversationCacheOptions = {
  currentConversationId: string | null;
  protectedConversationIds: ReadonlySet<string>;
  maxCount?: number;
  maxTextBytes?: number;
};

/**
 * 输入顺序即最近使用顺序。当前会话和运行中会话可以临时突破预算，其余项受双重上限约束。
 */
export function trimConversationCache(
  candidates: Conversation[],
  {
    currentConversationId,
    protectedConversationIds,
    maxCount = MAX_CACHED_CONVERSATIONS,
    maxTextBytes = MAX_CACHED_TEXT_BYTES,
  }: TrimConversationCacheOptions,
) {
  const unique: Conversation[] = [];
  const seen = new Set<string>();
  for (const conversation of candidates) {
    if (seen.has(conversation.id)) continue;
    seen.add(conversation.id);
    unique.push(conversation);
  }

  const requiredIds = new Set<string>(protectedConversationIds);
  if (currentConversationId) requiredIds.add(currentConversationId);

  const selectedIds = new Set<string>();
  let selectedBytes = 0;
  for (const conversation of unique) {
    if (!requiredIds.has(conversation.id)) continue;
    selectedIds.add(conversation.id);
    selectedBytes += estimateConversationTextBytes(conversation);
  }

  for (const conversation of unique) {
    if (selectedIds.has(conversation.id)) continue;
    const bytes = estimateConversationTextBytes(conversation);
    if (selectedIds.size >= maxCount || selectedBytes + bytes > maxTextBytes) continue;
    selectedIds.add(conversation.id);
    selectedBytes += bytes;
  }

  return unique.filter((conversation) => selectedIds.has(conversation.id));
}
