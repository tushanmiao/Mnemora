---
id: knowledge-capture
name: 知识整理
description: 将对话、决定和零散笔记转化为可复用的知识条目、操作指南、决策记录或问答，并保留背景与来源线索。
version: 1.0.0
license: MIT
compatibility: 当前适配版只生成结构化 Markdown，不连接 Notion，也不会自动写入 Mnemora 记忆或知识库。
triggers:
  - /capture
  - /knowledge
argument-hint: "<知识类型、受众或使用场景>"
metadata:
  mnemora:
    source-repository: https://github.com/openai/skills
    source-path: skills/.curated/notion-knowledge-capture/SKILL.md
    source-revision: 49f948faa9258a0c61caceaf225e179651397431
    attribution: "Knowledge Capture by Notion Labs, Inc., licensed under MIT and distributed in openai/skills."
    adapted: true
    adaptation-notes: 保留知识捕获的分类、结构化和来源追踪方法；移除 Notion MCP、数据库模板、页面创建及更新操作。
---

# 知识整理

先判断用户要沉淀的知识类型，再选择结构。当前 Skill 只生成正文，除非另有可用工具，否则不声称已经保存。

## 类型选择

- **概念条目**：定义、背景、原理、例子、边界、相关概念。
- **操作指南**：目标、前置条件、步骤、验证、失败处理。
- **决策记录**：背景、选项、决定、理由、影响、复审条件。
- **问答**：问题、简短答案、详细说明、例外和来源。
- **学习笔记**：摘要、关键概念、证据、疑问和下一步。

## 提取规则

1. 区分事实、决定、意见、推测和行动项。
2. 对决定保留备选方案、取舍理由和结果，不只记录最终答案。
3. 对操作说明保留前置条件、异常分支和验证方式。
4. 标注来源链接、文档、页码或对话背景；来源不明时明确写出。
5. 删除寒暄和重复过程，但不能删除影响结论的限制条件与分歧。

输出应能脱离当前聊天独立阅读，并包含适合后续更新的“待确认”或“变更记录”部分。
