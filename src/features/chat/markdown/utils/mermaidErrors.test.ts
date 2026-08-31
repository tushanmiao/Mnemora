import { describe, expect, it } from "vitest";
import { explainMermaidError, formatMermaidErrorSummary } from "./mermaidErrors";

/**
 * 全部是 mermaid 11.17.0 的真实报文，由 scripts/probe-mermaid-errors.mjs 采集。
 * 刻意不手写近似样本——期望列表的措辞一改，映射就会静默失效。
 */
const REAL_ERRORS = {
  parens: "Parse error on line 2:\ngraph TD\n  A[MCP 主机 (Host)]\n-------------------^\nExpecting 'SQE', 'DOUBLECIRCLEEND', 'PE', '-)', 'STADIUMEND', 'SUBROUTINEEND', 'PIPE', 'CYLINDEREND', 'DIAMOND_STOP', 'TAGEND', 'TRAPEND', 'INVTRAPEND', 'UNICODE_TEXT', 'TEXT', 'TAGSTART', got 'PS'",
  brackets: "Parse error on line 2:\ngraph TD\n  A[数组 [0] 元素]\n---------------^\nExpecting 'SQE', 'DOUBLECIRCLEEND', 'PE', '-)', 'STADIUMEND', 'SUBROUTINEEND', 'PIPE', 'CYLINDEREND', 'DIAMOND_STOP', 'TAGEND', 'TRAPEND', 'INVTRAPEND', 'UNICODE_TEXT', 'TEXT', 'TAGSTART', got 'SQS'",
  strAfterPipe: "Parse error on line 2:\n...art TD\n  A -->|\"x\"|\" Chat\n----------------------^\nExpecting 'SPACE', 'AMP', 'COLON', 'DOWN', 'DEFAULT', 'NUM', 'COMMA', 'NODE_STRING', 'BRKT', 'MINUS', 'MULT', 'UNICODE_TEXT', got 'STR'",
  spacedId: "Parse error on line 2:\n...TD\n  A -->|\"x\"|Deep Note Run\n----------------------^\nExpecting 'SEMI', 'NEWLINE', 'EOF', 'AMP', 'START_LINK', 'LINK', 'LINK_ID', got 'NODE_STRING'",
  badArrow: "Parse error on line 2:\ngraph TD\n  A -> B\n------------^\nExpecting 'SEMI', 'NEWLINE', 'EOF', 'AMP', 'START_LINK', 'LINK', 'LINK_ID', got 'MINUS'",
  erKey: "Parse error on line 3:\n...    string id PK_FK\n  }\n----------------------^\nExpecting 'ATTRIBUTE_WORD', '?', got 'BLOCK_STOP'",
  noType: "No diagram type detected matching given configuration for text: A --> B",
  badHeader: "Parse error on line 1:\nflowchartTD\n  A -->\n^\nExpecting 'NEWLINE', 'SPACE', 'GRAPH', got 'NODE_STRING'",
} as const;

describe("explainMermaidError", () => {
  it("识别未加引号的圆括号并给出加引号的改法", () => {
    const { hint, line } = explainMermaidError(new Error(REAL_ERRORS.parens));

    expect(line).toBe(2);
    expect(hint).toContain("圆括号");
    expect(hint).toContain("双引号");
  });

  it("识别标签里的方括号", () => {
    expect(explainMermaidError(new Error(REAL_ERRORS.brackets)).hint).toContain("方括号");
  });

  it("识别竖线闭合后多出的引号", () => {
    expect(explainMermaidError(new Error(REAL_ERRORS.strAfterPipe)).hint).toContain("竖线");
  });

  it("把带空格的节点 ID 和写错的箭头区分开", () => {
    // 两者期望列表完全相同，只有 got 标记不同，必须靠它区分。
    expect(explainMermaidError(new Error(REAL_ERRORS.spacedId)).hint).toContain("空格");
    expect(explainMermaidError(new Error(REAL_ERRORS.badArrow)).hint).toContain("箭头");
    expect(explainMermaidError(new Error(REAL_ERRORS.spacedId)).hint)
      .not.toBe(explainMermaidError(new Error(REAL_ERRORS.badArrow)).hint);
  });

  it("识别 erDiagram 复合键", () => {
    const { hint, line } = explainMermaidError(new Error(REAL_ERRORS.erKey));

    expect(line).toBe(3);
    expect(hint).toContain("PK, FK");
  });

  it("识别缺少图型声明和图型拼写错误", () => {
    expect(explainMermaidError(new Error(REAL_ERRORS.noType)).hint).toContain("图型声明");
    expect(explainMermaidError(new Error(REAL_ERRORS.noType)).line).toBeUndefined();
    expect(explainMermaidError(new Error(REAL_ERRORS.badHeader)).hint).toContain("空格");
  });

  it("始终保留原始报文", () => {
    for (const raw of Object.values(REAL_ERRORS)) {
      expect(explainMermaidError(new Error(raw)).raw).toBe(raw);
    }
  });

  it("认不出来时不编造解释", () => {
    const { hint, raw } = explainMermaidError(new Error("Something entirely unexpected"));

    expect(hint).toBe("");
    expect(raw).toBe("Something entirely unexpected");
  });

  it("容忍非 Error 与空输入", () => {
    expect(explainMermaidError("裸字符串").raw).toBe("裸字符串");
    expect(explainMermaidError(undefined).raw).toBe("Mermaid 图表解析失败。");
    expect(explainMermaidError("").hint).toBe("");
  });
});

describe("formatMermaidErrorSummary", () => {
  it("带上行号和提示", () => {
    const summary = formatMermaidErrorSummary(explainMermaidError(new Error(REAL_ERRORS.parens)));

    expect(summary).toMatch(/^第 2 行：/);
    expect(summary).toContain("圆括号");
  });

  it("认不出来时引导用户看原始报错", () => {
    expect(formatMermaidErrorSummary(explainMermaidError(new Error("unknown")))).toContain("原始报错");
  });

  it("让自动修复过仍失败这件事可见", () => {
    const summary = formatMermaidErrorSummary(
      explainMermaidError(new Error(REAL_ERRORS.spacedId)),
      ["quote-bracket-labels", "split-er-compound-keys"],
    );

    expect(summary).toContain("已自动修正 2 处");
  });
});
