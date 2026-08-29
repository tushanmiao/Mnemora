/**
 * 将 Markdown 消息投影为适合朗读的自然文本。
 *
 * 这是显示内容的语义投影，不修改原始消息：代码、Mermaid 和公式默认
 * 不朗读，链接保留可见标题，列表和表格保留其文字内容；行内公式只
 * 用“数学公式”提示，不把 TeX 源码逐字念出。
 */
export function extractSpeakableText(markdown: string): string {
  const lines: string[] = [];
  let inFence = false;
  let inDisplayMath = false;

  for (const rawLine of markdown.replace(/\r\n?/g, "\n").split("\n")) {
    const line = rawLine.trim();
    if (/^(`{3,}|~{3,})/.test(line)) {
      inFence = !inFence;
      continue;
    }
    if (inFence || !line) continue;

    // Display formulas can span several Markdown lines. Do not feed their
    // TeX source to the system voice; inline formulas are handled below.
    if (inDisplayMath) {
      if (line.includes("$$") || line.includes("\\]")) inDisplayMath = false;
      continue;
    }
    if (line.startsWith("$$")) {
      if ((line.match(/\$\$/g)?.length ?? 0) < 2) inDisplayMath = true;
      continue;
    }
    if (line.startsWith("\\[")) {
      if (!line.includes("\\]")) inDisplayMath = true;
      continue;
    }

    // Markdown 表格的分隔行没有可朗读信息。
    if (/^\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?$/.test(line)) continue;

    const projected = line
      .replace(/^#{1,6}\s+/, "")
      .replace(/^>\s?/, "")
      .replace(/^(?:[*+-]|\d+[.)])\s+/, "")
      .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
      .replace(/\[\^([^\]]+)\]:\s*/, "")
      .replace(/`([^`]+)`/g, "$1")
      // 公式的源码通常会被系统 TTS 逐字念出，默认只报出其存在。
      .replace(/\$\$[\s\S]*?\$\$/g, "数学公式")
      .replace(/(?<!\\)\$[^$\n]+(?<!\\)\$/g, "数学公式")
      .replace(/<[^>]+>/g, "")
      .replace(/\|/g, "，")
      .replace(/[ *_~]+/g, " ")
      .replace(/\s+/g, " ")
      .trim();

    if (projected) lines.push(projected);
  }

  return lines.join("。 ").replace(/。+([。！？!?])/g, "$1").trim();
}

/** 选区来自已渲染 DOM，已经是可见文字；这里只做边界和空白规范化。 */
export function normalizeSelectedSpeechText(text: string, maxCharacters = 20_000): string {
  return text
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, maxCharacters);
}

/** Web Speech 对超长 utterance 的实现差异很大，按自然停顿切成有限片段。 */
export function splitSpeechText(text: string, maxCharacters = 280): string[] {
  const normalized = text.replace(/\s+/g, " ").trim();
  if (!normalized) return [];
  const chunks: string[] = [];
  let rest = normalized;

  while (rest.length > maxCharacters) {
    const candidate = rest.slice(0, maxCharacters);
    const boundary = Math.max(
      candidate.lastIndexOf("。"),
      candidate.lastIndexOf("！"),
      candidate.lastIndexOf("？"),
      candidate.lastIndexOf("!"),
      candidate.lastIndexOf("?"),
      candidate.lastIndexOf("，"),
      candidate.lastIndexOf(","),
      candidate.lastIndexOf(" "),
    );
    const cut = boundary > Math.floor(maxCharacters * 0.45) ? boundary + 1 : maxCharacters;
    chunks.push(rest.slice(0, cut).trim());
    rest = rest.slice(cut).trim();
  }
  if (rest) chunks.push(rest);
  return chunks;
}

export function detectSpeechLanguage(text: string): string {
  return /[\u4e00-\u9fff]/.test(text) ? "zh-CN" : "en-US";
}
