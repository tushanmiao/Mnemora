// 采集各类语法错误的真实 jison 报文，用来给 explainMermaidError 建立映射。
//   node --import ./scripts/mermaid-loader.mjs scripts/probe-mermaid-errors.mjs
const { default: mermaid } = await import("mermaid");
mermaid.initialize({ startOnLoad: false, securityLevel: "loose", suppressErrorRendering: true });

const cases = [
  ["未加引号的圆括号", 'graph TD\n  A[MCP 主机 (Host) - AI 应用]'],
  ["未加引号的全角括号", "graph TD\n  A[验证响应状态（status）]"],
  ["未加引号的尖括号", "graph TD\n  A[类型 <T> 泛型]"],
  ["未加引号的冒号", "graph TD\n  A[标签: 说明]"],
  ["未加引号的逗号", "graph TD\n  A[甲, 乙, 丙]"],
  ["未加引号的方括号嵌套", "graph TD\n  A[数组 [0] 元素]"],
  ["竖线后多引号", 'flowchart TD\n  A -->|"说明"|" Chat'],
  ["多词节点 id", 'flowchart TD\n  A -->|"说明"|Deep Note Run'],
  ["erDiagram 复合键连写", "erDiagram\n  A {\n    string id PK_FK\n  }"],
  ["行尾分号", "graph TD\n  A --> B;\n  B --> C;"],
  ["缺少图型声明", "  A --> B"],
  ["未知图型", "flowchartTD\n  A --> B"],
  ["未闭合方括号", "graph TD\n  A[未闭合"],
  ["未闭合引号", 'graph TD\n  A["未闭合]'],
  ["箭头写错", "graph TD\n  A -> B"],
  ["subgraph 未 end", "graph TD\n  subgraph S\n    A"],
];

for (const [label, code] of cases) {
  try {
    await mermaid.parse(code);
    console.log(`【通过】${label}`);
  } catch (error) {
    const raw = (error?.message ?? String(error)).replace(/\s+/g, " ").trim();
    console.log(`【失败】${label}\n        ${raw.slice(0, 260)}\n`);
  }
}
