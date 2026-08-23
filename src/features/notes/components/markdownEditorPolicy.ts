/**
 * Large generated notes use the browser's native text surface. CodeMirror is
 * retained for ordinary notes, but WebView2 can fail to paint its virtual line
 * layer after a large document is revealed or resized.
 */
export const MARKDOWN_RICH_EDITOR_MAX_CHARS = 32 * 1024;

export function shouldUsePlainTextNoteEditor(length: number, paintFailed = false) {
  return paintFailed || length > MARKDOWN_RICH_EDITOR_MAX_CHARS;
}
