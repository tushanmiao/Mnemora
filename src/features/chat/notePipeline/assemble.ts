import type { DeepNoteOutline, DeepNoteSection } from "./outlineSchema";

export interface DraftedSection {
  section: DeepNoteSection;
  markdown: string;
  failed?: boolean;
}

export interface AssembledDeepNote {
  title: string;
  content: string;
  warnings: string[];
}

function headingLevels(markdown: string): number[] {
  return markdown.split("\n").flatMap((line) => {
    const match = /^(#{1,6})\s+/.exec(line);
    return match ? [match[1].length] : [];
  });
}

function selfCheckCount(markdown: string): number {
  const heading = /^#{2,4}\s+.*自检/m.exec(markdown);
  if (!heading?.index && heading?.index !== 0) return 0;
  const rest = markdown.slice(heading.index + heading[0].length);
  const nextHeading = /^#{1,4}\s+/m.exec(rest);
  const block = nextHeading ? rest.slice(0, nextHeading.index) : rest;
  return block.split("\n").filter((line) => /^\s*(?:[-*]\s+|\d+[.)]\s+)/.test(line)).length;
}

export function assembleDeepNote(
  outline: DeepNoteOutline,
  drafts: DraftedSection[],
  draft = false,
): AssembledDeepNote {
  const warnings: string[] = [];
  const title = draft ? `${outline.title}（草稿）` : outline.title;
  const headings = new Set<string>();
  let previousLevel = 1;
  for (const item of drafts) {
    const markdown = item.markdown.trim();
    if (!markdown) warnings.push(`章节“${item.section.heading}”为空。`);
    if (item.failed || markdown.includes("[本章生成失败")) warnings.push(`章节“${item.section.heading}”生成失败。`);
    // 组装阶段会为 needsSupplement 章节追加固定来源尾注，确保正文与数据层双轨标注。
    for (const line of markdown.split("\n")) {
      const match = /^#{2,6}\s+(.+)$/.exec(line);
      if (match) {
        const normalized = match[1].trim().toLowerCase();
        if (headings.has(normalized)) warnings.push(`重复标题：${match[1].trim()}。`);
        headings.add(normalized);
      }
    }
    for (const level of headingLevels(markdown)) {
      if (level > previousLevel + 1) warnings.push(`标题层级从 H${previousLevel} 跳到 H${level}。`);
      previousLevel = level;
    }
  }
  const body = drafts.map((item) => {
    const markdown = item.markdown.trim();
    if (!markdown) return "";
    const sourceLabels = [
      item.section.sourceMessageIds.length > 0
        ? `源自本次对话（${item.section.sourceMessageIds.length} 个消息锚点）`
        : "源自本次对话",
      item.section.needsSupplement ? "AI 补充背景" : "",
    ].filter(Boolean);
    return `${markdown}\n\n> 来源：${sourceLabels.join("；")}`;
  }).filter(Boolean).join("\n\n");
  const content = [`# ${title}`, outline.summary.trim(), body].filter(Boolean).join("\n\n").trim();
  const mermaidOpenings = (content.match(/```mermaid\b/g) ?? []).length;
  const fences = (content.match(/```/g) ?? []).length;
  if (mermaidOpenings > 0 && fences % 2 !== 0) warnings.push("Mermaid 代码围栏未闭合。");
  const questions = selfCheckCount(content);
  if (questions < 3) warnings.push(`自检问题不足 3 题（当前 ${questions} 题）。`);
  return { title, content, warnings: [...new Set(warnings)] };
}

export function sectionTail(markdown: string, maxChars = 500): string {
  const text = markdown.trim();
  return [...text].slice(-maxChars).join("");
}
