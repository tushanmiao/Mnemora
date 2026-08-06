# Mnemora

Mnemora 是一个面向个人学习与知识工作的桌面 AI 工作台，用于对话、Agent 任务、文献阅读和笔记整理。

项目使用 Tauri 2、Rust、React 和 TypeScript 构建，目前提供 Windows x64 版本。

## 下载

当前版本：**0.1.7**

[下载最新版本](https://github.com/tushanmiao/Mnemora/releases/latest)

- 推荐使用 `Mnemora_0.1.7_x64-setup.exe`
- MSI 安装包可用于备用安装
- 应用内更新使用 Tauri 签名校验

安装包暂未进行 Windows 代码签名，因此系统可能显示“未知发布者”或 SmartScreen 提示。

## 主要功能

- 支持 OpenAI、Anthropic、Gemini、自定义 Provider 和中转服务
- 支持流式对话、思考内容、重试、停止、编辑和重新生成
- Chat 默认具备 Agent 能力，可按模型能力运行工具、Skill 和多轮工作流
- Agent 工作过程默认在完成后折叠，也可以展开查看思考、工具和执行记录
- 支持 PDF 文献导入、阅读、导航、批注和关联笔记
- 支持 Markdown 笔记、会话转笔记和知识整理
- 支持 Skill 导入、启用、卸载和按需加载
- 提供用量统计、请求调试和内存诊断
- 包含英语学习、词典和复习功能

当当前模型不支持工具或视觉能力时，Mnemora 会保留普通 Chat，并明确提示缺少的能力，不会自动替换用户选择的模型。

## 数据与隐私

会话、笔记、文献、Skill 和设置默认保存在本机。API Key 使用系统凭据存储，不写入普通设置文件。

Mnemora 只会向用户配置的模型服务发送完成请求所需的数据。请求调试默认关闭，启用后保存的记录也会进行脱敏和数量限制。

## 开发

需要 Node.js、Rust、WebView2 和 Tauri 2 所需的 Windows 构建工具。

```powershell
npm install
npm run tauri dev
```

常用检查：

```powershell
npm test
npm run build
cd src-tauri
cargo check --lib
cargo test --lib
```

Release 构建：

```powershell
npm run tauri build
```

自动更新签名配置见 [Tauri Updater 发布说明](docs/release/tauri-updater.md)。

## 技术栈

- Tauri 2 / Rust
- React 19 / TypeScript / Vite
- PDF.js / react-markdown / Mermaid / KaTeX
- JSON 本地存储 / 系统凭据管理

## 项目状态

Mnemora 仍在持续开发，目前主要面向 Windows x64。问题反馈和功能建议请提交到 [GitHub Issues](https://github.com/tushanmiao/Mnemora/issues)。

仓库根目录暂未提供统一的 `LICENSE` 文件。除明确附带许可证的第三方内容外，请勿默认复制或再分发项目源码。
