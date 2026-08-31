/**
 * 渲染前的确定性 Mermaid 修复。
 *
 * 存在的理由：改 prompt 只能约束以后生成的图，救不了已经落盘的笔记。用户本地
 * 已有的图里就有模型漏加引号的，那些笔记早就写完入库了，只有在渲染前做一次
 * 无损修复才能追溯生效。
 *
 * 设计取向是保守：宁可漏修，不可改坏。每条规则都要求「原本一定渲染失败」，
 * 所以对本来能渲染的图不会产生任何改动。第三方工具（sopaco/mermaid-fixer）
 * 的做法是把 `B{验证响应状态(status)}` 直接改写成 `B{验证响应状态_Status}`，
 * 那是把语义删掉换个下划线；这里只加引号，原文一个字不动。
 */

/**
 * 需要加引号才能过词法分析的字符，全部实测确认过。
 *
 * 只有 ASCII 圆括号：它会被当成圆角节点 `(...)` 的开头。经实测，全角括号（）、
 * 尖括号 <>、冒号、逗号和行尾分号在方括号标签里都是合法的，所以刻意不碰——
 * 最小改动，避免给本来就能渲染的图产生无意义的差异。
 *
 * 标签里的方括号同样会报错（`got 'SQS'`），但那是本质歧义：`A[数组 [0] 元素]`
 * 无法在不猜测的前提下确定标签边界，一行多个节点时更是如此。那类交给错误
 * 提示层解释。
 */
const NEEDS_QUOTING = /[()]/;

/**
 * `[` 之后紧跟这些字符时是别的节点形状（`[[子程序]]`、`[(数据库)]`、
 * `[/平行四边形/]`、`[\反向\]`），闭合符号也不同，交给上游处理。
 */
const COMPOUND_SHAPE_OPENERS = new Set(["[", "(", "/", "\\"]);

/** 这些行首关键字后面的 `[` 不是节点标签，不能碰。 */
const SKIPPED_LINE_PREFIX = /^\s*(?:%%|click\b|style\b|classDef\b|class\b|linkStyle\b|direction\b|subgraph\b|end\b|accTitle\b|accDescr\b)/;

/** 只有 flowchart / graph 用 `ID[标签]` 这种写法，其他图型的 `[` 含义不同。 */
const FLOWCHART_HEADER = /^\s*(?:flowchart|graph)\b/;

const ER_HEADER = /^\s*erDiagram\b/;

/**
 * erDiagram 属性行的复合键。mermaid 的键位只接受逗号分隔的多个键，`PK_FK`
 * 和 `PK FK` 都会解析失败——两者都实测确认过。模型倾向写下划线连写。
 */
const ER_COMPOUND_KEY = /^(\s*\S+\s+\S+\s+)(PK|FK|UK)_(PK|FK|UK)(\s*(?:"[^"]*")?\s*)$/;

export type MermaidRepairResult = {
  source: string;
  /** 应用过的规则名，按首次生效顺序。空数组表示原文未被改动。 */
  repairs: string[];
};

/**
 * 判断整段图是否为 flowchart。跳过空行、注释和 init 指令后看第一行声明。
 */
function isFlowchart(source: string) {
  for (const line of source.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("%%")) continue;
    return FLOWCHART_HEADER.test(trimmed);
  }
  return false;
}

/**
 * 给 flowchart 里含圆括号的方括号标签补上引号。
 *
 * 只处理 `[` 单字符开头的形状，只在标签尚未被引号完整包裹时动手，标签内已经
 * 有裸引号的一律放过——那种情况下补引号会产生新的歧义，宁可让它继续报错，
 * 由错误提示层去解释。
 */
function quoteBracketLabels(source: string) {
  let changed = false;
  const repaired = source.split("\n").map((line) => {
    if (SKIPPED_LINE_PREFIX.test(line)) return line;

    let output = "";
    let cursor = 0;
    while (cursor < line.length) {
      const open = line.indexOf("[", cursor);
      if (open === -1) break;

      const next = line[open + 1];
      if (next !== undefined && COMPOUND_SHAPE_OPENERS.has(next)) {
        output += line.slice(cursor, open + 2);
        cursor = open + 2;
        continue;
      }

      const close = line.indexOf("]", open + 1);
      if (close === -1) break;

      const label = line.slice(open + 1, close);
      const trimmed = label.trim();
      const alreadyQuoted = trimmed.length >= 2 && trimmed.startsWith('"') && trimmed.endsWith('"');
      // 标签里还有 `[` 说明边界无法确定（嵌套方括号），一律放过。
      if (!alreadyQuoted && !label.includes('"') && !label.includes("[") && NEEDS_QUOTING.test(label)) {
        output += `${line.slice(cursor, open + 1)}"${label}"`;
        changed = true;
      } else {
        output += line.slice(cursor, close);
      }
      cursor = close;
    }
    return output + line.slice(cursor);
  }).join("\n");

  return changed ? repaired : null;
}

/** 把 `text run_id PK_FK` 改成 `text run_id PK, FK`。 */
function splitErCompoundKeys(source: string) {
  let changed = false;
  const repaired = source.split("\n").map((line) => {
    const match = ER_COMPOUND_KEY.exec(line);
    if (!match) return line;
    changed = true;
    return `${match[1]}${match[2]}, ${match[3]}${match[4]}`;
  }).join("\n");

  return changed ? repaired : null;
}

/**
 * 在把源码交给 Mermaid 之前跑一遍无损修复。返回值带上生效的规则名，方便
 * 出错时告诉用户「已经自动修过这些，仍然失败」。
 *
 * 刻意没做的一条：竖线闭合后多余的引号（`-->|"说明"|" 目标`）。删掉引号只在
 * 目标是单个标识符时有效；真实语料里那张图的目标是「Deep Note Run」这类多词
 * 短语，节点 id 不允许带空格，要修就得凭空造一个 id 并把原文挪进标签——那是
 * 改写语义而不是修语法。这条留给 prompt 层预防和错误提示层解释。
 */
export function repairMermaidSource(source: string): MermaidRepairResult {
  const repairs: string[] = [];
  let current = source;

  if (isFlowchart(current)) {
    const quoted = quoteBracketLabels(current);
    if (quoted !== null) {
      current = quoted;
      repairs.push("quote-bracket-labels");
    }
  } else if (ER_HEADER.test(current.trimStart())) {
    const split = splitErCompoundKeys(current);
    if (split !== null) {
      current = split;
      repairs.push("split-er-compound-keys");
    }
  }

  return { source: current, repairs };
}
