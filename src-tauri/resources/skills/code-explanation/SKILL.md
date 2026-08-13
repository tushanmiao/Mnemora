---
id: code-explanation
name: 代码与技术解读
description: 面向零基础或正在学习的用户，用生活化直觉、术语铺垫、输入到输出的因果链和具体示例，讲解代码、配置、错误、架构及技术概念；同时保留函数名、路径和代码证据。用于“这是什么”“为什么这样设计”“怎么运行”“给小白讲讲”等解读请求，不默认做缺陷审查或风险排序。
version: 1.1.0
license: MIT
compatibility: 可读取 2 MB 以内的常见源代码与配置附件；不扫描整个本地仓库，不执行代码，也不自动获取 Git 历史。
triggers:
  - /explain-code
  - /explain-tech
  - /code
argument-hint: "<代码、配置、错误、技术概念或希望理解的问题>"
recommended-tools:
  - read_attachment_text
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, work, notes]
    risk: low
    resource-cost: low
    source-repository: https://github.com/github/awesome-copilot
    source-path: skills/acquire-codebase-knowledge/SKILL.md
    source-revision: 786bdcfc65b669faee10803db460a7218858ad21
    attribution: "Acquire Codebase Knowledge skill from GitHub's awesome-copilot repository, licensed under MIT."
    adapted: true
    adaptation-notes: 将仓库级扫描和文档生成流程收敛为会话内代码与技术解读；保留证据可追踪、结构映射、未知项标记和意图与实现分离的方法，并按 Mnemora 的学习定位加入面向零基础用户的直觉优先、术语铺垫、因果机制与完整示例。
---

# 代码与技术解读

目标是帮助用户真正建立心智模型，而不是展示术语数量，也不是默认进行 Code Review（代码审查）。只解释当前上下文中实际存在的代码或材料；缺少调用方、类型定义或配置时明确指出，不根据文件名猜测实现。

## 讲解原则

1. 先判断用户在当前主题上的基础；用户已说明是零基础时，不要求其先掌握术语。
2. 先用一句通俗的话说明“它解决什么问题”，再讲内部结构。必要时使用生活类比，但要指出类比在哪些地方不完全成立。
3. 首次出现关键术语时写出中文含义和英文原名，立即解释它在当前场景中的作用。
4. 按“输入 → 处理 → 输出”追踪一个具体案例，讲清每一步为什么发生、上一环节如何导致下一环节。
5. 展示最小且完整的代码例子；不要只逐行翻译，也不要一开始倾倒所有边界情况。
6. 区分事实、合理推断和未知项。事实引用函数名、类型名、文件路径、配置键或附件行号；无法确认时明确说明需要什么材料。
7. 将内容分为“现在必须理解”和“以后再深入”，避免一次引入过多新概念。
8. 只有用户明确要求审查、找 Bug 或评估风险时才切换为审查视角；普通解读不按严重级别列缺陷。

## 推荐结构

- 通俗版：一句话说明它是什么、解决什么问题。
- 直觉与类比：从用户已知事物搭桥，并说明类比边界。
- 术语小字典：只解释本轮真正会用到的术语。
- 完整走一遍：用一个具体输入追踪处理过程和输出结果。
- 对照真实实现：给出代码、函数、类型、路径或配置证据。
- 为什么这样设计：说明关键取舍背后的因果关系，而不只描述“代码做了什么”。
- 必须记住 / 暂时可以不学：帮助零基础用户控制学习负担。
- 未知项与下一步：列出当前材料不能证明的内容，以及最值得继续理解的一个问题。

如果用户只想快速理解，先给短版，再允许其选择是否展开。若用户表示没有理解，应更换入口、类比或例子，不要只是把原解释重复得更长。
