# 20-DeepNote P2：文件系统化与影子写

本轮完成 P2-1 至 P2-16，并把 P2-17 推进到阶段 2。笔记现在具备磁盘目录表示，但在真实环境完成稳定性观察前，SQLite `library_notes.content` 仍是权威正文。

## 1. v17 数据模型与目录布局

v17 为 `library_notes` 增加目录状态字段，建立 `note_attachments`，清理并约束 `note_sources` 重复关系。每篇笔记对应：

```text
library/notes/<note-id>/
  note.md
  meta.json
  pipeline.json
  attachments/
  versions/
```

目录通过同级 staging、文件同步与原子 rename 更新。旧库在后台分批补建目录，不阻塞启动。

## 2. 双写、附件与渲染

- 新建、编辑、AI 改写和 DeepNote 提交都会刷新目录影子；
- DeepNote 持久化调整为先完成文件准备，再开启 SQLite 写事务，避免持锁复制大附件；
- 隐藏的 `localFiles` 会话在提交后移交并清理；普通可见会话使用复制语义，避免笔记生成破坏聊天附件；
- `note.md` 使用 `attachments/...` 相对链接，笔记预览和编辑器按笔记目录生成受控 asset URL；
- chat Markdown 的原有 URL 安全规则没有被放宽；
- assetProtocol 只新增 `library/notes/**` 范围。

## 3. 导出、同步、备份与回收

新增完整笔记目录导出；Obsidian 同步允许映射到目录内的 `note.md`，同时继续拒绝绝对路径和 `..` 穿越。备份覆盖整个 library 目录树。孤儿笔记目录先移动到带宽限期的回收区，不直接删除。

新库不再创建或写入 `note_pipeline_outputs`。旧库中的过渡表暂留，等待 P2-16 随正式版本发布后再通过后续迁移删除。

## 4. 阶段 2 对账与放量门槛

后台维护会迁移旧笔记、逐笔比较 DB 正文与 `note.md`，并把 checked、mismatch、missing 计数写入 `note_shadow_reconciliation_runs`。不一致会产生结构化告警。

阶段 3 尚未开启。只有真实环境跑满一个完整使用周期且 mismatch/missing 持续为零，才可以把文件切换为权威源；单元测试全绿不替代该门槛。

## 5. 验证

- Rust 全量测试：427 passed；
- Rust 全目标检查：`cargo check --all-targets` 通过；
- TypeScript：`npx tsc --noEmit` 通过；
- 前端测试：226 passed；
- `git diff --check` 通过。
