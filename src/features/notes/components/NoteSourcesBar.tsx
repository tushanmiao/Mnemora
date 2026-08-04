import { useEffect, useMemo, useState } from "react";
import { Bot, Link2, Unlink } from "lucide-react";
import { listLibraryNoteSources } from "../../library/api/library";
import type { NoteSource } from "../../library/types";

export function NoteSourcesBar({
  noteId,
  onOpenConversation,
}: {
  noteId: string;
  onOpenConversation?: (conversationId: string, messageId: string | null) => void;
}) {
  const [sources, setSources] = useState<NoteSource[]>([]);

  useEffect(() => {
    let disposed = false;
    void listLibraryNoteSources(noteId)
      .then((next) => {
        if (!disposed) setSources(next);
      })
      .catch(() => {
        if (!disposed) setSources([]);
      });
    return () => {
      disposed = true;
    };
  }, [noteId]);

  const items = useMemo(() => {
    const result: Array<{
      key: string;
      kind: "conversation" | "deleted" | "supplement";
      text: string;
      conversationId?: string;
      messageId?: string | null;
    }> = [];
    const conversations = new Map<string, { count: number; messageId: string | null }>();
    let deletedCount = 0;
    let supplementCount = 0;
    for (const source of sources) {
      if (source.origin === "aiSupplement") {
        supplementCount += 1;
      } else if (!source.conversationId) {
        deletedCount += 1;
      } else {
        const current = conversations.get(source.conversationId);
        conversations.set(source.conversationId, {
          count: (current?.count ?? 0) + 1,
          messageId: current?.messageId ?? source.messageId,
        });
      }
    }
    for (const [conversationId, value] of conversations) {
      result.push({
        key: `conversation:${conversationId}`,
        kind: "conversation",
        text: `来源对话 ${conversationId.slice(0, 8)} · ${value.count} 个锚点`,
        conversationId,
        messageId: value.messageId,
      });
    }
    if (deletedCount > 0) {
      result.push({
        key: "deleted",
        kind: "deleted",
        text: `原会话已删除 · ${deletedCount} 个失效锚点`,
      });
    }
    if (supplementCount > 0) {
      result.push({
        key: "supplement",
        kind: "supplement",
        text: `AI 补充背景 · ${supplementCount} 个章节`,
      });
    }
    return result;
  }, [sources]);

  if (items.length === 0) return null;

  return (
    <div className="note-sources-bar" aria-label="笔记来源">
      <strong><Link2 size={13} />来源</strong>
      {items.map((item) => item.conversationId && onOpenConversation ? (
        <button
          className={`note-source-chip note-source-${item.kind}`}
          type="button"
          key={item.key}
          title="打开来源对话"
          onClick={() => onOpenConversation(item.conversationId!, item.messageId ?? null)}
        >
          <Link2 size={12} />{item.text}
        </button>
      ) : (
        <span className={`note-source-chip note-source-${item.kind}`} key={item.key}>
          {item.kind === "deleted" ? <Unlink size={12} /> : item.kind === "supplement" ? <Bot size={12} /> : <Link2 size={12} />}
          {item.text}
        </span>
      ))}
    </div>
  );
}
