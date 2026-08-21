---
id: diagram
name: 图表与 Mermaid 设计
description: 把流程、依赖、状态变化、架构、时序或比较关系转换为可读的 Mermaid 图，并选择最小但足够表达关系的图型；适合“画流程图”“展示架构”“生成 Mermaid”等请求，不为简单事实强行加图。
version: 1.0.0
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
    adaptation-notes: 保留图型选择、语法正确性和复杂关系可视化原则；适配 Mnemora 彩色主题、安全渲染、零基础解释和 Artifact 交付。
---

# 图表与 Mermaid 设计

先判断图表是否比短段落或表格更容易看懂。只有存在三个以上节点、依赖、分支、状态或时间顺序时才优先画图。

## 图型选择

- 流程和分支：`flowchart`。
- 组件调用或消息顺序：`sequenceDiagram`。
- 状态迁移：`stateDiagram-v2`。
- 数据实体关系：`erDiagram`。
- 时间安排：`timeline` 或 `gantt`。
- 类和接口：`classDiagram`。

## 生成要求

1. 先提炼节点与关系，再写 Mermaid；不要把整段原文塞进节点。
2. 节点标题用短语，详细解释放在图后正文。
3. 关系方向应与阅读顺序一致；交叉线过多时拆图。
4. 需要颜色时使用少量语义 `classDef`，同一颜色保持同一含义；不能只靠颜色传达状态。
5. 不使用 `click`、外链图片、HTML 标签或依赖宽松安全级别的语法。
6. 生成后检查括号、引号、节点 ID、子图结束标记和图型关键字。
7. 适合独立查看的结果可调用 `present_artifact(kind="mermaid")`；正文仍说明图中最重要的关系。

## 完成标准

图应在不阅读长解释时也能看出主体、方向和关键分支，并能在 Mnemora 明暗主题中保持可读。
