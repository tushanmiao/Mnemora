import { describe, expect, it } from "vitest";
import { repairMermaidSource } from "./mermaidRepair";

describe("repairMermaidSource", () => {
  it("quotes the exact label class that fails to parse in real notes", () => {
    // 用户真实数据里的原样一行：括号让 mermaid 把 ( 当成圆角节点开头。
    const source = [
      "graph TD",
      "    A[MCP 主机 (Host) - AI 应用<br/>如 Claude Desktop / Visual Studio Code]",
    ].join("\n");

    const { source: repaired, repairs } = repairMermaidSource(source);

    expect(repairs).toEqual(["quote-bracket-labels"]);
    expect(repaired).toBe([
      "graph TD",
      '    A["MCP 主机 (Host) - AI 应用<br/>如 Claude Desktop / Visual Studio Code"]',
    ].join("\n"));
  });

  it("repairs every bracket label on a line and keeps the arrow intact", () => {
    const { source: repaired } = repairMermaidSource(
      "flowchart LR\n  A[读取 (source)] --> B[写入 (sink)]",
    );

    expect(repaired).toBe('flowchart LR\n  A["读取 (source)"] --> B["写入 (sink)"]');
  });

  it("leaves characters that mermaid already accepts unquoted", () => {
    // 实测：全角括号、尖括号、冒号、逗号、行尾分号在方括号标签里都合法。
    // 最小改动原则——本来能渲染的图不产生任何差异。
    for (const source of [
      "graph TD\n  A[验证响应状态（status）]",
      "graph TD\n  A[类型 <T> 泛型]",
      "graph TD\n  A[标签: 说明]",
      "graph TD\n  A[甲, 乙, 丙]",
      "graph TD\n  A --> B;",
    ]) {
      expect(repairMermaidSource(source)).toEqual({ source, repairs: [] });
    }
  });

  it("refuses nested brackets whose label boundary is ambiguous", () => {
    // A[数组 [0] 元素] 无法在不猜测的前提下确定边界，一行多节点时更是如此。
    for (const source of [
      "graph TD\n  A[数组 [0] 元素 (x)]",
      "graph TD\n  A[数组 [0]] --> B[值 (v)]",
    ]) {
      expect(repairMermaidSource(source).source).toContain("[0]");
    }
  });

  it("leaves diagrams that already parse untouched", () => {
    for (const source of [
      // 已经加过引号
      'graph TD\n  A["MCP 主机 (Host)"]',
      // 标签里没有括号，本来就合法
      "graph TD\n  A[纯文本节点] --> B[另一个节点<br/>第二行]",
      // 边标签，不是节点标签
      "graph TD\n  A -->|判断 (是)| B",
    ]) {
      expect(repairMermaidSource(source)).toEqual({ source, repairs: [] });
    }
  });

  it("does not touch非 flowchart 图型里含义不同的方括号", () => {
    for (const source of [
      "sequenceDiagram\n  Note over A: 调用 (同步)",
      "erDiagram\n  USER {\n    string name PK, FK\n  }",
      "mindmap\n  root((核心))\n    分支 (说明)",
      "gantt\n  section 阶段 (一)\n  任务 :a1, 2024-01-01, 3d",
    ]) {
      expect(repairMermaidSource(source)).toEqual({ source, repairs: [] });
    }
  });

  it("skips compound node shapes whose closing token differs", () => {
    for (const source of [
      "graph TD\n  A[[子程序 (sub)]]",
      "graph TD\n  A[(数据库 (db))]",
      "graph TD\n  A[/平行四边形 (io)/]",
      "graph TD\n  A[\\反向 (rev)\\]",
    ]) {
      expect(repairMermaidSource(source)).toEqual({ source, repairs: [] });
    }
  });

  it("skips directive and styling lines that use brackets differently", () => {
    for (const source of [
      "graph TD\n  %% 注释里提到 A[标签 (x)]",
      "graph TD\n  style A fill:#fff\n  classDef big font-size:20px",
      "graph TD\n  click A href\n  linkStyle 0 stroke:#000",
      "graph TD\n  subgraph 分组 (一)\n  end",
    ]) {
      expect(repairMermaidSource(source)).toEqual({ source, repairs: [] });
    }
  });

  it("splits erDiagram compound keys that mermaid rejects", () => {
    // 真实数据里的原样片段：PK_FK 和 PK FK 都实测解析失败，只有 PK, FK 通过。
    const { source: repaired, repairs } = repairMermaidSource([
      "erDiagram",
      "    note_pipeline_nodes {",
      "      text run_id PK_FK",
      "      integer plan_version PK",
      "      text node_id PK",
      "    }",
    ].join("\n"));

    expect(repairs).toEqual(["split-er-compound-keys"]);
    expect(repaired).toContain("text run_id PK, FK");
    expect(repaired).toContain("integer plan_version PK");
  });

  it("keeps erDiagram comments and single keys as they are", () => {
    for (const source of [
      'erDiagram\n  A {\n    text id PK "主键"\n  }',
      "erDiagram\n  A {\n    text id PK\n  }",
      "erDiagram\n  A {\n    text id\n  }",
      // 关系行里出现的下划线名字不能被误伤
      "erDiagram\n  note_pipeline_runs ||--o{ note_pipeline_nodes : schedules",
    ]) {
      expect(repairMermaidSource(source)).toEqual({ source, repairs: [] });
    }
  });

  it("does not apply the ER rule to flowcharts or vice versa", () => {
    const flowchartWithErLikeLine = "flowchart TD\n  A[text run_id PK_FK]";
    expect(repairMermaidSource(flowchartWithErLikeLine).repairs).toEqual([]);
  });

  it("leaves labels containing bare quotes alone rather than guessing", () => {
    // 补引号会产生新歧义，交给错误提示层解释比改坏更好。
    const source = 'graph TD\n  A[他说 "你好" (真的)]';

    expect(repairMermaidSource(source)).toEqual({ source, repairs: [] });
  });

  it("tolerates unbalanced brackets without dropping text", () => {
    for (const source of [
      "graph TD\n  A[未闭合 (x",
      "graph TD\n  A]多余的右括号 (x)",
    ]) {
      expect(repairMermaidSource(source).source).toBe(source);
    }
  });
});
