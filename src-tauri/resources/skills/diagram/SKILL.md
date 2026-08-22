---
id: diagram
name: 图表与 Mermaid 设计
description: 把流程、依赖、状态变化、架构、时序或比较关系转换为可读的 Mermaid 图，并选择最小但足够表达关系的图型；适合“画流程图”“展示架构”“生成 Mermaid”等请求，不为简单事实强行加图。
version: 1.1.0
license: MIT
compatibility: 纯提示词技能；可使用 present_artifact 交付 Mermaid，并应保持图表可被 Mnemora 安全渲染。
triggers: [/diagram, /mermaid, /flowchart]
argument-hint: "<需要可视化的关系、流程或结构>"
recommended-tools: [present_artifact]
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, work, notes]
    risk: low
    resource-cost: low
    source-repository: https://github.com/github/awesome-copilot
    source-path: skills/draw-io-diagram-generator/SKILL.md
    source-revision: 3b2c4fb913430e6ec7ebc52a22e2aefc40015245
    attribution: "Mermaid diagram guidance from GitHub's awesome-copilot repository, licensed under MIT."
    adapted: true
    adaptation-notes: 保留图型选择、语法正确性和复杂关系可视化原则；结合 Mermaid 官方图型能力与 Mnemora 真实渲染支持，新增 mindmap、journey、requirement、ER 和有来源数值的统计图选型矩阵。
---

# 图表与 Mermaid 设计

先判断图表是否比短段落或表格更容易看懂。只有存在三个以上节点、依赖、分支、状态或时间顺序时才优先画图。

## 图型选择

先问“读者需要一眼回答什么问题”，再选择图型。不要因为 `flowchart` 最熟悉，就把所有关系都画成流程图。

| 要回答的问题 | 首选图型 | 使用边界 |
| --- | --- | --- |
| 步骤如何推进、条件如何分支、依赖如何传递 | `flowchart` | 最通用；交叉线过多时拆图 |
| 概念如何分层、知识如何分类 | `mindmap` | 只表达层级，不承担严格时序 |
| 对象会经历哪些状态、何时迁移 | `stateDiagram-v2` | 状态必须互斥且转换条件明确 |
| 多个角色如何按时间交互 | `sequenceDiagram` | 适合 API、Agent、Writer/Validator 调用链 |
| 任务或章节如何沿时间排期 | `gantt` / `timeline` | 必须存在真实时间、阶段或里程碑 |
| 实体、主外键和基数是什么 | `erDiagram` | 数据库与领域模型首选 |
| 类型、接口、继承或组合关系是什么 | `classDiagram` | 不要替代真实 ER 图 |
| 用户或任务从起点到终点经历什么 | `journey` | 适合体验、执行路径和痛点 |
| 需求、约束与验收标准如何对应 | `requirementDiagram` | 适合正式需求和测试追踪 |
| 有真实数值时如何比较趋势或占比 | `xychart-beta` / `pie` | 禁止为了图形效果编造数据 |
| 版本或分支如何演进 | `gitGraph` | 只用于真实版本/分支关系 |

深度笔记不追求“图越多越好”。短笔记通常 0–2 张，长笔记通常 2–5 张；只有不同图真正回答不同认知问题时才增加数量。重复的流程图应合并或改用更贴合语义的图型。

## 生成要求

1. 先提炼节点与关系，再写 Mermaid；不要把整段原文塞进节点。
2. 节点标题用短语，详细解释放在图后正文。
3. 关系方向应与阅读顺序一致；交叉线过多时拆图。
4. 需要颜色时使用少量语义 `classDef`，同一颜色保持同一含义；不能只靠颜色传达状态。
5. 不使用 `click`、外链图片、HTML 标签或依赖宽松安全级别的语法。
6. 生成后检查括号、引号、节点 ID、子图结束标记和图型关键字。
7. 适合独立查看的结果可调用 `present_artifact(kind="mermaid")`；正文仍说明图中最重要的关系。
8. 写入可直接渲染的 Markdown 正文时，`mermaid` 围栏必须位于正文顶层；不要把它包在 `markdown` / `md` / `text` 源码围栏或四反引号示例中。
9. 统计图只使用来源中出现的可核验数值，并在图后说明口径；没有数值就改用表格或关系图。
10. 每张图后用一段话指出读图结论，避免把图当装饰。

## 完成标准

图应在不阅读长解释时也能看出主体、方向和关键分支，并能在 Mnemora 明暗主题中保持可读。
