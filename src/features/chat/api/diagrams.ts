import { invoke } from "@tauri-apps/api/core";

/**
 * 把图表字节写到用户选定的路径。
 *
 * 走 Rust 而不是 `<a download>`：WebView2 不认 `download` 属性，点了没反应。
 * 路径由前端的保存对话框给出，写盘与父目录校验都在 Rust 侧做。
 *
 * @param path 保存对话框返回的绝对路径。
 * @param dataBase64 文件字节的 base64（PNG 位图或 SVG 文本）。
 */
export async function exportDiagramFile(path: string, dataBase64: string): Promise<void> {
  await invoke("export_diagram_file", { path, dataBase64 });
}
