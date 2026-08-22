import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export type LocalNoteSourceResult = {
  conversationId: string;
  fileNames: string[];
  attachmentCount: number;
};

const LOCAL_NOTE_EXTENSIONS = [
  "md", "markdown", "txt", "rst", "csv", "json", "jsonl", "xml", "html", "htm",
  "css", "js", "jsx", "ts", "tsx", "rs", "py", "java", "c", "h", "cpp", "hpp",
  "cs", "go", "rb", "php", "swift", "kt", "kts", "sql", "yaml", "yml", "toml",
  "ini", "cfg", "conf", "log", "tex", "bib", "pdf", "docx", "png", "jpg", "jpeg",
  "webp", "gif", "xlsx",
];

export function localNoteSourceExtensions() {
  return [...LOCAL_NOTE_EXTENSIONS];
}

export async function chooseLocalNoteSourceFiles() {
  if (!isTauri()) return [] as string[];
  const selected = await open({
    title: "选择用于生成笔记的本地文件",
    multiple: true,
    directory: false,
    filters: [{ name: "可生成笔记的文件", extensions: LOCAL_NOTE_EXTENSIONS }],
  });
  return typeof selected === "string" ? [selected] : selected ?? [];
}

export function prepareLocalNoteSource(paths: string[]) {
  if (!isTauri()) return Promise.reject(new Error("本地文件笔记需要在桌面应用中运行。"));
  return invoke<LocalNoteSourceResult>("prepare_local_note_source", { paths });
}

export function discardLocalNoteSource(conversationId: string) {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("discard_local_note_source", { conversationId });
}
