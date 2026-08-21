---
id: obsidian-bases
name: Obsidian Bases
description: 设计和解释 Obsidian Bases 的视图、筛选、排序、属性、公式与汇总，用结构化查询组织一组 Markdown 笔记；仅在用户明确使用 Obsidian Bases 或 .base 文件时启用。
version: 1.0.0
license: MIT
compatibility: 不运行 Obsidian；可读取现有 .base 文本并交付可复制的定义。
triggers: [/bases, /obsidian-bases]
argument-hint: "<要建立的笔记数据库视图或现有 .base 内容>"
recommended-tools: [workspace_glob, workspace_read, present_artifact]
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, work, notes]
    risk: low
    resource-cost: low
    source-repository: https://github.com/kepano/obsidian-skills
    source-path: skills/obsidian-bases/SKILL.md
    source-revision: a1dc48e68138490d522c04cbf5822214c6eb1202
    attribution: "Obsidian Bases skill by kepano/obsidian-skills, licensed under MIT."
    adapted: true
    adaptation-notes: 保留 Bases 的属性、过滤、公式和视图组织原则；适配 Mnemora 只读分析与 Artifact 交付。
---

# Obsidian Bases

先明确目标集合、稳定属性和用户真正要回答的问题，再设计视图。

1. 用少量稳定属性表达数据，不把可推导内容重复存储。
2. 筛选条件应直接对应目标集合；复杂条件拆开解释优先级。
3. 排序先放最能帮助决策的字段，时间字段注明升降序含义。
4. 公式必须说明输入属性、空值行为和输出类型。
5. 同一 Base 中的表格、卡片或列表视图应服务不同阅读任务，不复制同一信息层级。
6. 修改已有 `.base` 前先读取其完整小文件；没有写入工具时交付完整可替换文本，不声称已经保存。
