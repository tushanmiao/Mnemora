/**
 * 把 mermaid 的 jison 报错翻译成可执行的中文提示。
 *
 * 原始报文长这样：
 *   Parse error on line 2: ... Expecting 'SQE', 'DOUBLECIRCLEEND', 'PE', '-)',
 *   'STADIUMEND', 'SUBROUTINEEND', 'PIPE', ... got 'PS'
 * 期望列表是语法表的内部符号，对写笔记的人毫无意义，能定位问题的信息只有
 * 末尾的 got 标记和行号。
 *
 * 下面每条映射都由 scripts/probe-mermaid-errors.mjs 实测得到，不是推断。探针
 * 同时否掉了几个想当然的猜测：全角括号（）、尖括号 <>、冒号、逗号、行尾分号
 * 在方括号标签里其实都合法，所以这里没有对应条目。
 *
 * 原始报文一律保留，由调用方决定折叠还是展开——参照 lvy010/mermaid-validator
 * 的 { error, rawError } 双层结构。
 */

export type MermaidErrorExplanation = {
  /** 一句话说清哪里错了、怎么改。没有匹配到已知模式时为空串。 */
  hint: string;
  /** 出错行号，从 1 开始。解析不出来时为 undefined。 */
  line?: number;
  /** mermaid 原始报文，始终保留以便复制排查。 */
  raw: string;
};

type Pattern = {
  /** 匹配 got 标记，或整段报文的特征。 */
  test: (raw: string) => boolean;
  hint: string;
};

const PATTERNS: Pattern[] = [
  {
    test: (raw) => /got 'PS'/.test(raw),
    hint: "节点标签里的半角圆括号 ( 被当成了圆角节点的开头。把整个标签用英文双引号包起来，例如 A[\"主机 (Host)\"]。",
  },
  {
    test: (raw) => /got 'SQS'/.test(raw),
    hint: "节点标签里出现了半角方括号 [，被当成下一个节点的开头。改用全角［］，或去掉方括号后用双引号包住整个标签。",
  },
  {
    test: (raw) => /got 'STR'/.test(raw),
    hint: "边标签的竖线闭合之后多了一个引号。写成 A -->|\"说明\"| B，竖线后面不要再加引号。",
  },
  {
    test: (raw) => /Expecting[\s\S]*'LINK_ID'[\s\S]*got 'NODE_STRING'/.test(raw),
    hint: "箭头右侧的节点 ID 含有空格。节点 ID 不能带空格，多词短语要写成 ID[\"多词 短语\"] 的形式。",
  },
  {
    test: (raw) => /Expecting[\s\S]*'LINK_ID'[\s\S]*got 'MINUS'/.test(raw),
    hint: "箭头写法不对。实线箭头是 -->，虚线是 -.->，粗线是 ==>，单个 -> 不被接受。",
  },
  {
    test: (raw) => /got 'BLOCK_STOP'/.test(raw) && /ATTRIBUTE_WORD/.test(raw),
    hint: "erDiagram 的复合键写法不对。多个键要用英文逗号分隔，写成 PK, FK，不能写 PK_FK 或 PK FK。",
  },
  {
    test: (raw) => /No diagram type detected/i.test(raw),
    hint: "缺少图型声明。首行要写明图型，例如 flowchart TD、sequenceDiagram、erDiagram。",
  },
  {
    test: (raw) => /Expecting[\s\S]*'GRAPH'[\s\S]*got 'NODE_STRING'/.test(raw),
    hint: "图型声明拼写有误，关键字和方向之间需要空格，例如 flowchart TD 而不是 flowchartTD。",
  },
];

/** 从 "Parse error on line 2:" 里取行号。 */
function parseLine(raw: string) {
  const match = /Parse error on line (\d+)/i.exec(raw);
  if (!match) return undefined;
  const line = Number(match[1]);
  return Number.isFinite(line) ? line : undefined;
}

/**
 * 翻译一条 mermaid 错误。认不出来就只回原文，不编造解释——给错的方向比不给
 * 方向更浪费时间。
 */
export function explainMermaidError(reason: unknown): MermaidErrorExplanation {
  const raw = (reason instanceof Error ? reason.message : String(reason ?? "")).trim();
  if (!raw) return { hint: "", raw: "Mermaid 图表解析失败。" };

  const matched = PATTERNS.find((pattern) => pattern.test(raw));
  return { hint: matched?.hint ?? "", line: parseLine(raw), raw };
}

/**
 * 拼出一行给用户看的摘要。带上行号和已经自动修过的规则，让「修了还是失败」
 * 这件事对用户是可见的，而不是让他们怀疑修复根本没跑。
 */
export function formatMermaidErrorSummary(
  explanation: MermaidErrorExplanation,
  appliedRepairs: readonly string[] = [],
) {
  const at = explanation.line ? `第 ${explanation.line} 行：` : "";
  const body = explanation.hint || "Mermaid 图表解析失败，展开下方原始报错可查看解析器给出的位置。";
  const repaired = appliedRepairs.length > 0
    ? `（已自动修正 ${appliedRepairs.length} 处常见笔误后仍然失败）`
    : "";
  return `${at}${body}${repaired}`;
}
