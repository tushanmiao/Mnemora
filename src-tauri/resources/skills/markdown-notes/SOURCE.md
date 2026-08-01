# 上游来源

- 仓库：https://github.com/kepano/obsidian-skills
- 原始路径：`skills/obsidian-markdown/SKILL.md`
- 固定版本：`a1dc48e68138490d522c04cbf5822214c6eb1202`
- 许可证：MIT
- 许可证依据：仓库根目录 `LICENSE`，该 Skill 未声明不同许可证；许可证已原样保存在本目录。

## Mnemora 适配

吸收原 Skill 对结构化 Markdown、属性、引用和笔记组织的工作方法，但目标从 Obsidian 改为 Mnemora 当前的安全 CommonMark/GFM 渲染器。Mnemora 保留并适配 Mermaid 图表、脚注、学习提示块和可信文献引用；删除 Wikilink、Vault 嵌入、CSS class、脚本和其他尚未支持的 Obsidian 专用语法。Mermaid 只在进入可视区域后按需加载，渲染失败时保留源码，不阻断整条消息。
