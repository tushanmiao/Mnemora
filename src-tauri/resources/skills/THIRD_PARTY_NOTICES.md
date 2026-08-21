# Mnemora 内置 Skill 第三方声明

Mnemora 的内置 Skill 是经过适配的工作说明，不代表上游项目对 Mnemora 的认可或背书。每个 Skill 均固定到具体 Commit，并在自己的目录中保存：

- `SKILL.md`：适配后的 Mnemora 工作说明。
- `SOURCE.md`：仓库、原始路径、Commit、许可证依据和改编说明。
- `LICENSE.txt`：适用许可证副本；若仅借鉴工作流思想而未复制上游代码，则保存许可证边界与独立适配声明。

| Mnemora Skill | 上游项目 | 固定 Commit | 许可证 |
|---|---|---|---|
| PDF 阅读 | `openai/skills` | `33a75a7b572867072dc0674bee8e63e06c19e67b` | Apache-2.0 |
| 论文研究 | `xwmxcz/papers-skill` | `a64c2eda2c9fc182c96e1409cde267b262dbebde` | MIT |
| 科学证据分析 | `K-Dense-AI/scientific-agent-skills` | `831d49eb77eed3c792be2970921b46764012ef00` | MIT |
| Markdown 笔记 | `kepano/obsidian-skills` | `a1dc48e68138490d522c04cbf5822214c6eb1202` | MIT |
| 知识整理 | `openai/skills` / Notion Labs | `49f948faa9258a0c61caceaf225e179651397431` | MIT |
| 系统化调试 | `obra/superpowers` | `d884ae04edebef577e82ff7c4e143debd0bbec99` | MIT |
| 逐项追问与压力测试 | `mattpocock/skills` | `2ab958093e83e0ec752e6c1c5932da465bf23e0c` | MIT |
| 图片证据分析 | `github/awesome-copilot` | `786bdcfc65b669faee10803db460a7218858ad21` | MIT |
| Word 文档阅读 | `github/awesome-copilot` | `e4a1f57fd9d8c22d2a345d498fe6fde306c6456e` | MIT |
| Excel 表格分析 | `github/awesome-copilot` | `e4a1f57fd9d8c22d2a345d498fe6fde306c6456e` | MIT |
| 代码与技术解读 | `github/awesome-copilot` | `786bdcfc65b669faee10803db460a7218858ad21` | MIT |
| 问题框定 | `Nandansai08/skillz` | `6571a300abb8e49e7c7520896041734aede52c91` | MIT |
| 小白讲解 | `pjt222/agent-almanac` | `6345ef3b26a9ef4b3745a8e3875a0a8eb56b3a18` | MIT |
| 图表与 Mermaid 设计 | `github/awesome-copilot` | `3b2c4fb913430e6ec7ebc52a22e2aefc40015245` | MIT |
| Obsidian Markdown | `kepano/obsidian-skills` | `a1dc48e68138490d522c04cbf5822214c6eb1202` | MIT |
| Obsidian Bases | `kepano/obsidian-skills` | `a1dc48e68138490d522c04cbf5822214c6eb1202` | MIT |
| JSON Canvas | `kepano/obsidian-skills` | `a1dc48e68138490d522c04cbf5822214c6eb1202` | MIT |
| 深度研究与证据综合 | `github/awesome-copilot` | `05f4d6757e4765d88a52506fadf747c869368250` | MIT |
| 本地知识库检索 | `github/awesome-copilot` | `b8f38227480c9f3fe04d6496d4fcab9a880e5a15` | MIT |
| 工作区代码阅读 | `github/awesome-copilot` | `b8f38227480c9f3fe04d6496d4fcab9a880e5a15` | MIT |
| 结构化文档撰写 | `github/awesome-copilot` | `caab1f623bb68a330f294a11279597d7ae7be737` | MIT |
| 需求收敛与方案规划 | `mindfold-ai/Trellis` | `9914a7f2b1b61c0e3890be474f771f49aa115f88` | AGPL-3.0（仅工作流思想的独立中文适配） |

## 未纳入的候选

- Anthropic `pdf`、`docx`、`xlsx`：各目录的许可证明确受 Anthropic 服务协议约束，不作为 Mnemora 的跨供应商内置 Skill 分发。
- GitHub `awesome-copilot` 的完整 Word/Excel 转 Markdown 脚本没有原样引入。Mnemora 只适配其读取工作流，并使用有界、按需创建和立即释放的纯 Rust 解析器。
- 完整 Skill 合集：不整体打包，避免许可证混杂、无关依赖和内存/安装体积增长。
- `Trellis` 等 AGPL-3.0 项目：只吸收工作流思想，不复制代码；对应 Skill 保留独立适配声明。
