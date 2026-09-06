import { useI18n } from "../../../i18n/I18nProvider";

const english: Record<string, string> = {
  "整词": "Whole word", "重试编辑器": "Retry editor",
  "未插入的已保留图片": "Preserved images awaiting insertion", "恢复图片": "Recover image",
  "已另存为": "Saved as",
  "链接地址": "Link address", "行数": "Rows", "列数": "Columns", "图表类型": "Diagram type", "取消": "Cancel", "混合段落": "Mixed styles",
  "实时编辑": "Live", "源码": "Source", "阅读": "Read", "编辑模式": "Editing mode", "Markdown 格式": "Markdown formatting",
  "段落样式": "Paragraph style", "段落": "Paragraph", "正文": "Body", "粗体": "Bold", "斜体": "Italic", "删除线": "Strikethrough", "行内代码": "Inline code",
  "无序列表": "Bullet list", "有序列表": "Numbered list", "任务列表": "Task list", "引用": "Quote", "插入 Markdown": "Insert Markdown", "插入": "Insert",
  "链接": "Link", "表格": "Table", "代码块": "Code block", "公式": "Math", "脚注": "Footnote", "分隔线": "Divider", "提示块": "Callout",
  "高亮": "Highlight", "下划线": "Underline", "上标": "Superscript", "下标": "Subscript", "插入图片": "Insert image", "撤销": "Undo", "重做": "Redo",
  "查找替换": "Find and replace", "版本历史": "Version history", "保存笔记": "Save note", "更多操作": "More actions", "专注模式": "Focus mode", "自动换行": "Word wrap",
  "跳转行": "Go to line", "上移章节": "Move section up", "下移章节": "Move section down", "导出当前 Markdown": "Export current Markdown",
  "导出 Markdown 与附件": "Export Markdown with assets", "导出 HTML": "Export HTML", "关闭": "Close",
  "Markdown 笔记正文": "Markdown note body", "Markdown 纯文本恢复编辑器": "Markdown recovery editor", "Markdown 阅读": "Markdown reading", "Markdown 块预览": "Markdown block preview",
  "大文档 · 源码模式": "Large document · Source mode", "编辑器绘制失败 · 纯文本恢复模式": "Editor unavailable · Plain text recovery",
  "正在加载": "Loading", "图片保留中": "Preserving images", "正在保存": "Saving", "存在冲突": "Version conflict", "保存失败": "Save failed",
  "草稿已保留": "Draft preserved", "有未保存修改": "Unsaved changes", "已保存": "Saved",
  "表格操作": "Table actions", "在上方插入行": "Insert row above", "在下方插入行": "Insert row below", "在左侧插入列": "Insert column left", "在右侧插入列": "Insert column right",
  "左对齐": "Align left", "居中": "Align center", "右对齐": "Align right", "删除当前行": "Delete current row", "删除当前列": "Delete current column",
  "复制选中单元格": "Copy selected cells", "删除整表": "Delete table", "编辑表格源码": "Edit table source", "打开源码": "Open source",
  "表格超出可视编辑预算，打开源码": "Table exceeds the visual editing limit. Open source",
  "超出表格编辑预算；原文已保留，可在源码中粘贴。": "Table exceeds the editing limit. Original text is preserved; paste in Source mode.",
  "粘贴内容超限，原文已保留。": "Clipboard content exceeds the limit. Original text is preserved.",
  "完成块编辑": "Finish block editing", "编辑块源码": "Edit block source", "复制块源码": "Copy block source", "定位完整源码": "Locate in full source",
  "语言": "Language", "代码块语言": "Code block language", "图片替代文字": "Image alternative text", "替换图片": "Replace image", "块 Markdown 源码": "Block Markdown source",
  "切换任务状态": "Toggle task", "笔记版本历史": "Note version history", "关闭版本历史": "Close version history", "暂无历史版本": "No saved versions",
  "恢复此版本": "Restore this version", "固定版本": "Pin version", "导出版本": "Export version", "另存为新笔记": "Save as a new note", "固定 · ": "Pinned · ",
  "笔记版本冲突": "Note version conflict", "共同基线": "Common base", "本地草稿": "Local draft", "当前文件": "Current file",
  "保留本地": "Keep local", "采用文件": "Use current file", "导出草稿": "Export draft", "恢复草稿": "Recover draft", "恢复": "Recover", "导出": "Export", "丢弃": "Discard", "重试保存": "Retry save",
  "查找文本": "Find text", "替换文本": "Replacement text", "区分大小写": "Case sensitive", "正则": "Regex", "仅选区": "Selection only", "上一个": "Previous", "下一个": "Next", "替换": "Replace", "替换全部": "Replace all", "关闭查找": "Close search",
  "正则支持字符类、转义与锚点；不支持重复、分组和分支，最多 128 字符。": "Regex supports character classes, escapes and anchors; repetition, groups and alternatives are disabled. Maximum 128 characters.",
  "正文超过 500,000 字符或 2 MiB，插入已取消。": "Content exceeds 500,000 characters or 2 MiB. Insertion was cancelled.",
  "每次最多插入 10 张图片。": "Insert up to 10 images at a time.", "图片已保留，编辑视图已变化，请重新插入。": "The image is preserved, but the view changed. Insert it again.",
  "图片已保留，但插入位置已被修改，请重新插入。": "The image is preserved, but the insertion point changed. Insert it again.",
  "图片已保留，但原引用已变化，请重新选择。": "The image is preserved, but its reference changed. Select it again.",
  "正在加载编辑器": "Loading editor", "大纲": "Outline", "笔记大纲": "Note outline", "没有检测到标题。使用 “#” 开头的标题行会出现在这里。": "Headings will appear here. Use # to create one.",
  "网络图片仍需联网加载。": "Remote images still require a network connection.",
};

export function noteText(language: string, value: string): string {
  if (language !== "en") return value;
  if (english[value]) return english[value];
  const cell = /^(表头|第 (\d+) 行)，第 (\d+) 列$/.exec(value);
  if (cell) return `${cell[2] ? `Row ${cell[2]}` : "Header"}, column ${cell[3]}`;
  return value;
}
export function useNoteText() {
  const { language } = useI18n();
  return (value: string) => noteText(language, value);
}
