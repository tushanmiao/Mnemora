import type { DeepNoteOutline, DeepNoteSection } from "./outlineSchema";

export const ANALYST_SYSTEM_PROMPT = [
  "你是 Mnemora 深度学习笔记管线的分析师。只诊断对话并设计提纲，不写正文。",
  "识别用户追问、误解、自我修正、隐含前置知识、需要比较的相似概念和适合自检的问题。",
  "AI 可以建议补充通用背景，但不得把补充内容伪装成对话事实。",
  "只输出严格 JSON，不要 Markdown 代码围栏，不要解释。",
  "JSON 契约：",
  '{"title":"不含 # 的标题","summary":"1~3 句概览","weakPoints":["薄弱点"],"sections":[{"id":"sec-1","heading":"章节标题","kind":"prerequisite|concept|comparison|pitfall|example|summary|selfcheck","brief":"本章内容与依据","needsSupplement":false,"sourceMessageIds":["消息 ID"]}]}',
  "章节 1~40 个，id 唯一且只能用 ASCII 字母、数字、连字符和下划线（形如 sec-1，不要用中文或空格）；sourceMessageIds 只能引用输入中标出的消息 ID。",
].join("\n");

export const STRICT_JSON_RETRY_SUFFIX = [
  "上一次输出无法解析。现在必须只输出一个合法 JSON 对象。",
  "不要代码围栏、注释、前后缀文字、尾随逗号。字段必须完整且符合契约。",
].join("\n");

export function analystUserPrompt(transcript: string, adjustment = ""): string {
  return [
    adjustment.trim() ? `用户对提纲的补充要求：\n${adjustment.trim()}` : "",
    "请分析以下对话转写并输出提纲 JSON：",
    transcript,
  ].filter(Boolean).join("\n\n");
}

export function sectionSystemPrompt(): string {
  return [
    "你是 Mnemora 深度学习笔记的章节撰写者。只输出当前章节正文。",
    "以 ## 章节标题开头；内容自洽、具体、可复习，避免重复其他章节。",
    "输出会直接渲染为 Markdown，禁止用 ```markdown、```md 或四反引号包裹整章；Mermaid 必须是正文顶层的 ```mermaid 代码块。",
    "对话事实与 AI 补充必须分层。needsSupplement=true 时，在补充内容附近明确标注“AI 补充背景”，并提示建议进一步核实。",
    "复杂流程按需使用 Mermaid；数学公式统一使用 KaTeX 兼容的 Markdown：行内使用 $...$，独立公式使用 $$...$$，不要使用 ```math、```latex 或 ```tex 代码围栏；相似概念按需使用 Markdown 表格；示例应说明输入、过程、结果。",
    "Mermaid 语法硬约束，违反会直接导致渲染失败：节点标签只要含有 ( ) [ ] 这四个半角括号，必须整体用英文双引号包起来，例如 A[\"MCP 主机 (Host) - AI 应用<br/>如 Claude Desktop / Visual Studio Code\"]；不加引号时 ( 会被当成圆角节点的开头、[ 会被当成下一个节点的开头，立刻报错。换行只用 <br/>，不要用裸 <br> 或字面 \\n。边标签写成 A -->|说明| B，竖线闭合后不要再补引号；箭头右侧必须是不含空格的节点 ID，多词短语写成 ID[\"多词 短语\"]。erDiagram 复合键写 PK, FK 而不是 PK_FK 或 PK FK。",
    "不要输出全文 H1，不要写‘好的’或‘以下是’。",
  ].join("\n");
}

export function sectionUserPrompt({
  outline,
  section,
  transcript,
  previousTail,
}: {
  outline: DeepNoteOutline;
  section: DeepNoteSection;
  transcript: string;
  previousTail: string;
}): string {
  return [
    `全局标题：${outline.title}`,
    `全局概览：${outline.summary}`,
    `薄弱点：${outline.weakPoints.join("；") || "无显式薄弱点"}`,
    `全部章节：\n${outline.sections.map((item) => `- ${item.id} ${item.heading} (${item.kind})`).join("\n")}`,
    `当前章节：${JSON.stringify(section)}`,
    previousTail ? `前一章末段摘要：\n${previousTail}` : "",
    `对话转写：\n${transcript}`,
  ].filter(Boolean).join("\n\n");
}
