import { invoke } from "@tauri-apps/api/core";
import { createNoteReference } from "../../chat/utils/noteReferences";
import { canonicalMarkdown, utf16RangeToUtf8 } from "../editor/markdownRanges";
import type { NoteEditSession } from "./noteEditSession";

/** Freeze a committed identity. Rendered/ambiguous excerpts never invent offsets. */
export async function prepareNoteSelection(session: NoteEditSession, selectedText: string) {
  const generation = session.snapshot().generation;
  await session.save();
  const { base } = session.snapshot();
  const unchanged = () => session.snapshot().generation === generation && !session.dirty;
  if (!base || !unchanged()) throw new Error("NOTE_RANGE_STALE: 笔记已变化，请重新选择引用范围。");
  const reference = createNoteReference({ noteId: session.noteId, noteTitle: base.note.title,
    revisionHash: base.contentHash, noteVersion: base.noteVersion, selectedText });
  if (!reference) throw new Error("NOTE_RANGE_STALE: 引用内容为空。");
  const source = canonicalMarkdown(base.note.content), from = source.indexOf(reference.selectedText);
  if (from >= 0 && source.indexOf(reference.selectedText, from + 1) < 0) {
    const to = from + reference.selectedText.length, range = utf16RangeToUtf8(source, from, to);
    await invoke("note_editor_validate_selection", { noteId: session.noteId, noteVersion: base.noteVersion,
      contentHash: base.contentHash, ...range, selectedText: reference.selectedText });
    Object.assign(reference, range, { rangeEncoding: "utf8CanonicalLf",
      startLine: source.slice(0, from).split("\n").length, endLine: source.slice(0, to).split("\n").length });
  }
  if (!unchanged()) throw new Error("NOTE_RANGE_STALE: 笔记已变化，请重新选择引用范围。");
  return reference;
}
