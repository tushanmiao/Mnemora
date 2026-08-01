import { headingId } from "../utils/outline";

type HastNode = {
  type: string;
  tagName?: string;
  properties?: Record<string, unknown>;
  position?: { start?: { offset?: number } };
  children?: HastNode[];
};

function scopedDocumentId(messageId: string, value: string) {
  const safeMessageId = messageId.replace(/[^a-zA-Z0-9_-]/g, "-");
  const safeValue = value.replace(/[^a-zA-Z0-9_-]/g, "-");
  return `mnemora-doc-${safeMessageId}-${safeValue}`;
}

function visit(node: HastNode, messageId: string) {
  if (node.type === "element" && node.tagName) {
    const properties = node.properties ?? (node.properties = {});
    const offset = node.position?.start?.offset;
    if (/^h[1-6]$/.test(node.tagName) && typeof offset === "number") {
      properties.id = headingId(messageId, offset);
    }

    const id = typeof properties.id === "string" ? properties.id : "";
    if (id.startsWith("user-content-fn") || id === "footnote-label") {
      properties.id = scopedDocumentId(messageId, id);
    }
    if (properties.ariaDescribedBy === "footnote-label") {
      properties.ariaDescribedBy = scopedDocumentId(messageId, "footnote-label");
    }
    const href = typeof properties.href === "string" ? properties.href : "";
    if (href.startsWith("#user-content-fn")) {
      properties.href = `#${scopedDocumentId(messageId, href.slice(1))}`;
    }
  }
  node.children?.forEach((child) => visit(child, messageId));
}

/** 为标题和脚注增加消息级作用域，避免长对话中的重复 ID 冲突。 */
export function rehypeScopeDocument(messageId: string) {
  return () => (tree: unknown) => visit(tree as HastNode, messageId);
}
