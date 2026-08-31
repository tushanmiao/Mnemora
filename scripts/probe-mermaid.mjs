// 探针：逐行二分定位 mermaid 到底在哪一行失败，并验证候选修复。
//
// 给 mermaidRepair.ts 和 service.rs 的 lint_mermaid_syntax 加规则之前，先在这里
// 用真实解析器确认「命中即必然失败」，不要凭直觉写规则——已经有好几条想当然的
// 猜测在这里被否掉了。
//   node --import ./scripts/mermaid-loader.mjs scripts/probe-mermaid.mjs
const { default: mermaid } = await import("mermaid");
mermaid.initialize({ startOnLoad: false, securityLevel: "loose", suppressErrorRendering: true });

const check = async (label, code) => {
  try {
    await mermaid.parse(code);
    console.log(`  通过  ${label}`);
    return true;
  } catch (error) {
    const message = (error?.message ?? String(error)).split("\n").find((l) => /Expecting|Parse error|got/.test(l)) ?? "";
    console.log(`  失败  ${label}  ${message.trim().slice(0, 110)}`);
    return false;
  }
};

console.log("A. 竖线闭合后多一个引号");
await check('|"x"|" Chat  (原样)', 'flowchart TD\n  A -->|"生成结果"|" Chat');
await check('|"x"| Chat   (删掉多余引号)', 'flowchart TD\n  A -->|"生成结果"| Chat');
await check('|"x"|"Chat"   (右侧整体加引号)', 'flowchart TD\n  A -->|"生成结果"|"Chat"');
await check('|"x"|Chat    (无空格)', 'flowchart TD\n  A -->|"生成结果"|Chat');

console.log("\nB. subgraph 带引号 id");
await check('subgraph "X" ["X"]', 'flowchart TD\n  subgraph "Deep Note Run" ["Deep Note Run"]\n    S["a"]\n  end');
await check("subgraph X [\"X\"]", 'flowchart TD\n  subgraph Run ["Deep Note Run"]\n    S["a"]\n  end');

console.log("\nC. 字面 \\n 在引号标签里");
await check("标签含字面 \\n", 'flowchart TD\n  N["D 不加入当前 Run\\n（章节检查点）"]');
await check("标签含 <br/>", 'flowchart TD\n  N["D 不加入当前 Run<br/>（章节检查点）"]');

console.log("\nA2. 右侧目标带空格时怎么删");
await check('|"x"|"Deep Note Run"  (原样)', 'flowchart TD\n  A -->|"x"|"Deep Note Run"');
await check('|"x"|Deep Note Run   (两个引号都删)', 'flowchart TD\n  A -->|"x"|Deep Note Run');
await check('|"x"| Deep Note Run  (删引号留空格)', 'flowchart TD\n  A -->|"x"| Deep Note Run');
await check('-.->|"x"|"保持旧 Run 历史"', 'flowchart TD\n  A -.->|"x"|"保持旧 Run 历史"');
await check('-.->|"x"|保持旧 Run 历史', 'flowchart TD\n  A -.->|"x"|保持旧 Run 历史');

console.log("\nD. erDiagram 复合键");
await check("PK_FK", "erDiagram\n  A {\n    string id PK_FK\n  }");
await check("PK, FK", "erDiagram\n  A {\n    string id PK, FK\n  }");
await check("PK FK", "erDiagram\n  A {\n    string id PK FK\n  }");
