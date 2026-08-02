import { MarkdownMessage } from "../../chat/components/MarkdownMessage";

type MarkdownNotePreviewProps = {
  noteId: string;
  content: string;
};

/** 预览沿用 Chat 的安全 Markdown 能力；组件只在预览模式下挂载。 */
export default function MarkdownNotePreview({ noteId, content }: MarkdownNotePreviewProps) {
  return (
    <article className="notes-markdown-preview" aria-label="Markdown 预览">
      <MarkdownMessage content={content || "_空笔记_"} messageId={`note-${noteId}`} />
    </article>
  );
}
