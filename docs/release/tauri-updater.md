# Tauri Updater 发布说明

## 版本顺序

- `0.1.4` 没有 Updater，只能手动安装新版本。
- `0.1.5` 是首个包含签名 Updater 的基线版本，仍需手动安装。
- `0.1.6` 用于验证第一次真实的 `0.1.5 -> 0.1.6` 应用内升级。

## 本机签名密钥

私钥不在仓库中，当前保存在：

```text
%USERPROFILE%\.mnemora\updater.key
%USERPROFILE%\.mnemora\updater.key.password
```

公钥已经写入 `src-tauri/tauri.conf.json`。私钥和密码必须离线备份；丢失后，已经安装的版本将无法验证由新密钥签名的更新。

本地构建签名包前设置：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$env:USERPROFILE\.mnemora\updater.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = Get-Content "$env:USERPROFILE\.mnemora\updater.key.password" -Raw
npm run tauri build
```

不要把私钥、密码、环境变量输出或构建日志上传到公开位置。

## GitHub Secrets

在仓库 `Settings -> Secrets and variables -> Actions` 中增加：

```text
TAURI_SIGNING_PRIVATE_KEY
TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

- `TAURI_SIGNING_PRIVATE_KEY`：`updater.key` 的完整内容。
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：`updater.key.password` 的完整内容。

## 发布流程

1. 统一更新 `package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 和 `src-tauri/tauri.conf.json` 的版本。
2. 完成测试并提交代码。
3. 创建并推送版本标签，例如 `v0.1.5`。
4. GitHub Actions 构建 Windows x64 的 NSIS、MSI 和 Updater 签名产物。
5. 工作流生成草稿 Release，同时上传 `latest.json`、NSIS Updater 包和 `.sig`。
6. 核对资产和 `latest.json` 后再手动发布草稿，避免不完整 Release 被客户端读取。

客户端固定读取：

```text
https://github.com/tushanmiao/Mnemora/releases/latest/download/latest.json
```

## `0.1.5 -> 0.1.6` 验收

1. 手动安装已发布的 `0.1.5` NSIS 安装包。
2. 确认会话、供应商、API Key、PDF、批注和笔记都存在。
3. 发布带签名产物的稳定版 `0.1.6`。
4. 在 `0.1.5` 的“设置 -> 关于”中手动检查更新。
5. 确认显示版本、说明和下载进度，再点击“下载并安装”。
6. 确认旧进程退出、NSIS 被动覆盖安装、新版本重新启动。
7. 确认应用版本为 `0.1.6`，原有本地数据和系统凭据仍然可用。
8. 使用错误签名或篡改安装包做反向测试，客户端必须拒绝安装且 `0.1.5` 继续可用。

正常升级不是“先卸载并删除数据”，而是由签名 NSIS Updater 在旧进程退出后执行覆盖安装。只有安装损坏或安装器类型迁移时才考虑手动卸载。
