import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { flushNoteEditors } from "./noteEditSession";

export async function installNoteEditorCloseGuard() {
  if (!isTauri()) return;
  let closing = false;
  await listen<boolean>("mnemora://note-editor-close", async (event) => {
    if (closing) return;
    closing = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([flushNoteEditors(), new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error("笔记仍在保存，请稍后重试关闭窗口。")), 3000);
      })]);
      await invoke("note_editor_finish_close", { exit: event.payload });
    } catch (error) {
      window.alert(String(error));
    } finally {
      if (timer) clearTimeout(timer);
      closing = false;
    }
  });
  await invoke("note_editor_register_close");
}
