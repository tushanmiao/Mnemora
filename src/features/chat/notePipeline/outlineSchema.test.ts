import { describe, expect, it } from "vitest";
import { parseDeepNoteOutline, selectOutlineSections } from "./outlineSchema";

const valid = JSON.stringify({
  title: "MVCC",
  summary: "事务并发控制。",
  weakPoints: ["隔离级别"],
  sections: [
    { id: "sec-1", heading: "前置知识", kind: "prerequisite", brief: "解释隔离级别", needsSupplement: true, sourceMessageIds: ["message-1", "missing"] },
    { id: "sec-2", heading: "自检问题", kind: "selfcheck", brief: "检查理解", needsSupplement: false, sourceMessageIds: [] },
  ],
});

describe("parseDeepNoteOutline", () => {
  it("parses fenced JSON and drops unknown message ids", () => {
    const outline = parseDeepNoteOutline(`\`\`\`json\n${valid}\n\`\`\``, new Set(["message-1"]));
    expect(outline.sections[0].sourceMessageIds).toEqual(["message-1"]);
  });

  it("rejects invalid kinds and duplicate ids", () => {
    expect(() => parseDeepNoteOutline(valid.replace('"selfcheck"', '"unknown"'), new Set())).toThrow();
    expect(() => parseDeepNoteOutline(valid.replace('"sec-2"', '"sec-1"'), new Set())).toThrow(/重复/);
  });

  it("requires at least one selected section", () => {
    const outline = parseDeepNoteOutline(valid, new Set(["message-1"]));
    expect(selectOutlineSections(outline, new Set(["sec-2"])).sections).toHaveLength(1);
    expect(() => selectOutlineSections(outline, new Set())).toThrow(/至少/);
  });
});
