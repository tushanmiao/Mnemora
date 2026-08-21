---
id: json-canvas
name: JSON Canvas
description: 创建、解释或检查 JSON Canvas（.canvas）中的文本节点、文件节点、链接节点、分组和边，适合知识地图、概念关系图和 Obsidian Canvas 文件。
version: 1.0.0
license: MIT
compatibility: 不直接写入 Canvas 文件；可读取现有 JSON，并用 present_artifact(kind=json) 交付符合规范的内容。
triggers: [/canvas, /json-canvas]
argument-hint: "<要表达的节点、分组和关系>"
recommended-tools: [workspace_read, present_artifact]
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, work, notes]
    risk: low
    resource-cost: low
    source-repository: https://github.com/kepano/obsidian-skills
    source-path: skills/json-canvas/SKILL.md
    source-revision: a1dc48e68138490d522c04cbf5822214c6eb1202
    attribution: "JSON Canvas skill by kepano/obsidian-skills, licensed under MIT."
    adapted: true
    adaptation-notes: 保留开放 JSON Canvas 数据模型；适配 Mnemora 有界读取、JSON 校验和结构化 Artifact 交付。
---

# JSON Canvas

输出根对象 `{"nodes": [], "edges": []}`。每个 ID 必须唯一且稳定。

- 文本节点使用 `type: "text"` 和 `text`。
- 文件节点使用 `type: "file"` 和相对 `file` 路径；不要虚构文件。
- 链接节点使用 `type: "link"` 和有效 URL。
- 分组使用 `type: "group"`，通过坐标包围相关节点。
- 边必须引用存在的 `fromNode` 与 `toNode`；方向有意义时设置端点箭头。
- 坐标和尺寸使用有限数值，避免节点完全重叠；关系过密时分组或拆图。
- 交付前验证 JSON、ID 唯一性、边引用和路径；调用 `present_artifact(kind="json")` 可触发基础 JSON 校验。
