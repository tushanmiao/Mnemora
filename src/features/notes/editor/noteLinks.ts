import { invoke, isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { safeNoteAttachmentPath } from "../utils/noteMarkdownUrls";

export async function openNoteLink(noteId: string, href: string) {
  if (safeNoteAttachmentPath(href)) {
    if (isTauri()) await invoke("note_editor_open_attachment", { noteId, relativePath: href });
  } else if (/^(https?:|mailto:)/i.test(href)) {
    if (isTauri()) await openUrl(href);
    else window.open(href, "_blank", "noopener,noreferrer");
  }
}
