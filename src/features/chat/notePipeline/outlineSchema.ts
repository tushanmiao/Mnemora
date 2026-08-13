export const NOTE_SECTION_KINDS = [
  "prerequisite",
  "concept",
  "comparison",
  "pitfall",
  "example",
  "summary",
  "selfcheck",
] as const;

export type DeepNoteSectionKind = typeof NOTE_SECTION_KINDS[number];

export interface DeepNoteSection {
  id: string;
  heading: string;
  kind: DeepNoteSectionKind;
  brief: string;
  purpose?: string;
  dependsOn?: string[];
  evidenceRequirements?: string[];
  successCriteria?: string[];
  sourceScope?: string[];
  targetDepth?: string;
  allowAiSupplement?: boolean;
  needsSupplement: boolean;
  sourceMessageIds: string[];
}

export interface DeepNoteOutline {
  goal?: string;
  audience?: string;
  scope?: string;
  title: string;
  summary: string;
  weakPoints: string[];
  allowAiSupplement?: boolean;
  evidencePolicy?: string;
  sourceIds?: string[];
  sections: DeepNoteSection[];
}

export const MAX_DEEP_NOTE_SECTIONS = 40;
const MAX_TITLE_CHARS = 500;
const MAX_SECTION_ID_CHARS = 128;

function extractJsonObject(value: string): string {
  const trimmed = value.trim();
  const fenced = /^```(?:json)?\s*([\s\S]*?)\s*```$/i.exec(trimmed);
  const candidate = fenced?.[1]?.trim() ?? trimmed;
  const start = candidate.indexOf("{");
  const end = candidate.lastIndexOf("}");
  if (start < 0 || end <= start) throw new Error("分析师没有返回 JSON 对象。");
  return candidate.slice(start, end + 1);
}

function requiredText(value: unknown, label: string, maxChars: number): string {
  if (typeof value !== "string") throw new Error(`${label}必须是字符串。`);
  const normalized = value.trim();
  if (!normalized) throw new Error(`${label}不能为空。`);
  if ([...normalized].length > maxChars) throw new Error(`${label}过长。`);
  return normalized;
}

function stringList(value: unknown, label: string, maxItems = 100): string[] {
  if (!Array.isArray(value)) throw new Error(`${label}必须是数组。`);
  if (value.length > maxItems) throw new Error(`${label}项目过多。`);
  return value
    .filter((item): item is string => typeof item === "string")
    .map((item) => item.trim())
    .filter(Boolean);
}

export function parseDeepNoteOutline(
  raw: string,
  validMessageIds: ReadonlySet<string>,
): DeepNoteOutline {
  let value: unknown;
  try {
    value = JSON.parse(extractJsonObject(raw));
  } catch (error) {
    throw new Error(`提纲 JSON 解析失败：${error instanceof Error ? error.message : String(error)}`);
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("提纲必须是 JSON 对象。");
  }
  const record = value as Record<string, unknown>;
  const sectionsValue = record.sections;
  if (!Array.isArray(sectionsValue) || sectionsValue.length === 0) {
    throw new Error("提纲至少需要一个章节。");
  }
  if (sectionsValue.length > MAX_DEEP_NOTE_SECTIONS) {
    throw new Error(`提纲最多允许 ${MAX_DEEP_NOTE_SECTIONS} 个章节。`);
  }

  const ids = new Set<string>();
  const sections = sectionsValue.map((sectionValue, index): DeepNoteSection => {
    if (!sectionValue || typeof sectionValue !== "object" || Array.isArray(sectionValue)) {
      throw new Error(`第 ${index + 1} 个章节无效。`);
    }
    const section = sectionValue as Record<string, unknown>;
    const id = requiredText(section.id, `第 ${index + 1} 个章节 ID`, MAX_SECTION_ID_CHARS);
    if (ids.has(id)) throw new Error(`章节 ID 重复：${id}`);
    ids.add(id);
    const kind = section.kind;
    if (typeof kind !== "string" || !NOTE_SECTION_KINDS.includes(kind as DeepNoteSectionKind)) {
      throw new Error(`章节 ${id} 的 kind 无效。`);
    }
    const sourceMessageIds = stringList(
      section.sourceMessageIds ?? [],
      `章节 ${id} 的 sourceMessageIds`,
      200,
    ).filter((messageId) => validMessageIds.has(messageId));
    return {
      id,
      heading: requiredText(section.heading, `章节 ${id} 标题`, 300),
      kind: kind as DeepNoteSectionKind,
      brief: requiredText(section.brief, `章节 ${id} 简介`, 4_000),
      purpose: typeof section.purpose === "string" && section.purpose.trim()
        ? section.purpose.trim()
        : requiredText(section.brief, `章节 ${id} 简介`, 4_000),
      dependsOn: [...new Set(stringList(section.dependsOn ?? [], `章节 ${id} 依赖`, 40))],
      evidenceRequirements: [...new Set(stringList(
        section.evidenceRequirements ?? [],
        `章节 ${id} 证据要求`,
      ))],
      successCriteria: [...new Set(stringList(
        section.successCriteria ?? [],
        `章节 ${id} 成功标准`,
      ))],
      sourceScope: [...new Set(stringList(section.sourceScope ?? [], `章节 ${id} 来源范围`))],
      targetDepth: typeof section.targetDepth === "string" && section.targetDepth.trim()
        ? section.targetDepth.trim()
        : "standard",
      allowAiSupplement: section.allowAiSupplement === true || section.needsSupplement === true,
      needsSupplement: section.needsSupplement === true,
      sourceMessageIds: [...new Set(sourceMessageIds)],
    };
  });
  for (const section of sections) {
    if (section.dependsOn?.some((dependency) => dependency === section.id || !ids.has(dependency))) {
      throw new Error(`章节“${section.heading}”包含无效依赖。`);
    }
  }

  return {
    goal: typeof record.goal === "string" ? record.goal.trim() : "",
    audience: typeof record.audience === "string" ? record.audience.trim() : "",
    scope: typeof record.scope === "string" ? record.scope.trim() : "",
    title: requiredText(record.title, "笔记标题", MAX_TITLE_CHARS),
    summary: typeof record.summary === "string" ? record.summary.trim() : "",
    weakPoints: stringList(record.weakPoints ?? [], "weakPoints", 100),
    allowAiSupplement: record.allowAiSupplement === true,
    evidencePolicy: typeof record.evidencePolicy === "string" ? record.evidencePolicy.trim() : "",
    sourceIds: [...new Set(stringList(record.sourceIds ?? [], "sourceIds", 500))],
    sections,
  };
}

export function selectOutlineSections(
  outline: DeepNoteOutline,
  selectedSectionIds: ReadonlySet<string>,
): DeepNoteOutline {
  const sections = outline.sections.filter((section) => selectedSectionIds.has(section.id));
  if (sections.length === 0) throw new Error("请至少保留一个章节。");
  return { ...outline, sections };
}
