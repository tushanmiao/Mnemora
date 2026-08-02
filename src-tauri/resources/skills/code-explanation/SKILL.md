---
id: code-explanation
name: 代码解读
description: 以调用关系、数据流、状态变化、边界条件和可验证代码证据为主线，解释用户粘贴或作为文本附件提供的代码。
version: 1.0.0
license: MIT
compatibility: 可读取 2 MB 以内的常见源代码与配置附件；不扫描整个本地仓库，不执行代码，也不自动获取 Git 历史。
triggers:
  - /explain-code
  - /code
argument-hint: "<文件、函数、流程或希望理解的问题>"
recommended-tools:
  - read_attachment_text
metadata:
  mnemora:
    default-enabled: false
    supported-modes: [chat]
    risk: low
    resource-cost: low
    source-repository: https://github.com/github/awesome-copilot
    source-path: skills/acquire-codebase-knowledge/SKILL.md
    source-revision: 786bdcfc65b669faee10803db460a7218858ad21
    attribution: "Acquire Codebase Knowledge skill from GitHub's awesome-copilot repository, licensed under MIT."
    adapted: true
    adaptation-notes: 将仓库级扫描和文档生成流程收敛为会话内代码解读；保留证据可追踪、结构映射、未知项标记和意图与实现分离的方法。
---

# 代码解读

只解释当前上下文中实际存在的代码。缺少调用方、类型定义或配置时明确指出，不根据文件名猜测实现。

## 解读顺序

1. 说明文件或代码片段在当前已知系统中的职责。
2. 列出主要入口、关键类型、核心函数和它们之间的调用关系。
3. 沿一次典型输入追踪数据如何转换、校验、保存或返回。
4. 说明状态归属、生命周期、异步/并发边界和错误传播路径。
5. 标出关键条件、默认值、资源上限、安全约束和可能影响行为的配置。
6. 将无法从现有代码确认的意图标为“未知”或“需要补充文件”。

## 输出结构

- 一句话职责
- 结构与调用关系
- 典型执行流程
- 关键变量或类型
- 边界与错误处理
- 需要继续阅读的文件或问题

解释应引用函数名、类型名、代码片段或文本附件行号；避免只把代码逐行翻译成自然语言。
