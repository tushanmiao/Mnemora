# Mnemora 内置 Skill 第三方声明

Mnemora 的内置 Skill 是经过适配的工作说明，不代表上游项目对 Mnemora 的认可或背书。每个 Skill 均固定到具体 Commit，并在自己的目录中保存：

- `SKILL.md`：适配后的 Mnemora 工作说明。
- `SOURCE.md`：仓库、原始路径、Commit、许可证依据和改编说明。
- `LICENSE.txt`：适用许可证的完整副本。

| Mnemora Skill | 上游项目 | 固定 Commit | 许可证 |
|---|---|---|---|
| PDF 阅读 | `openai/skills` | `49f948faa9258a0c61caceaf225e179651397431` | Apache-2.0 |
| 论文研究 | `xwmxcz/papers-skill` | `a64c2eda2c9fc182c96e1409cde267b262dbebde` | MIT |
| 科学证据分析 | `K-Dense-AI/scientific-agent-skills` | `831d49eb77eed3c792be2970921b46764012ef00` | MIT |
| Markdown 笔记 | `kepano/obsidian-skills` | `a1dc48e68138490d522c04cbf5822214c6eb1202` | MIT |
| 知识整理 | `openai/skills` / Notion Labs | `49f948faa9258a0c61caceaf225e179651397431` | MIT |
| 代码审查 | `wshobson/agents` | `c4b82b0ad771190355eb8e204b1329732a18449a` | MIT |
| 系统化调试 | `obra/superpowers` | `d884ae04edebef577e82ff7c4e143debd0bbec99` | MIT |
| 图片证据分析 | `github/awesome-copilot` | `786bdcfc65b669faee10803db460a7218858ad21` | MIT |
| Word 文档阅读 | `github/awesome-copilot` | `786bdcfc65b669faee10803db460a7218858ad21` | MIT |
| Excel 表格分析 | `github/awesome-copilot` | `786bdcfc65b669faee10803db460a7218858ad21` | MIT |
| 代码解读 | `github/awesome-copilot` | `786bdcfc65b669faee10803db460a7218858ad21` | MIT |

## 未纳入的候选

- Anthropic `pdf`、`docx`、`xlsx`：各目录的许可证明确受 Anthropic 服务协议约束，不作为 Mnemora 的跨供应商内置 Skill 分发。
- GitHub `awesome-copilot` 的完整 Word/Excel 转 Markdown 脚本没有原样引入。Mnemora 只适配其读取工作流，并使用有界、按需创建和立即释放的纯 Rust 解析器。
- 完整 Skill 合集：不整体打包，避免许可证混杂、无关依赖和内存/安装体积增长。
