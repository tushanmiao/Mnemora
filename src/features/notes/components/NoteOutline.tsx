import { VList } from "virtua";
import type { MarkdownOutlineItem } from "../../chat/markdown/utils/outline";
import { useNoteText } from "../editor/noteText";

export function NoteOutline({ items, onJump }: { items: MarkdownOutlineItem[]; onJump: (item: MarkdownOutlineItem) => void }) {
  const nt = useNoteText();
  if (!items.length) return <p>{nt("没有检测到标题。使用 “#” 开头的标题行会出现在这里。")}</p>;
  return <VList className="note-outline-list" bufferSize={160} style={{ height: "100%" }}>{items.map((item) => <button
    type="button" key={item.id} title={item.title} style={{ paddingInlineStart: `${10 + (item.level - 1) * 13}px` }} onClick={() => onJump(item)}
  >{item.title}</button>)}</VList>;
}
