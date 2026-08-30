# 21-DeepNote P3：内容寻址、全局缓存与章节检查点

本轮完成 P3-2 至 P3-6。P3-1 已随 P0-10 完成；P3-7 保留发布门槛，P3-8 因没有确认到 FTS5 依赖而不执行。

## 1. 内容寻址与全局 digest 缓存

Chunk ID 改为正文内容的稳定哈希，相同内容即使位于不同 run、不同消息位置，也得到相同 ID。digest prompt 同时移除了位置、run 和消息编号等不影响语义的字段，策略版本提升以显式隔离旧缓存。

v18 将 `note_pipeline_chunk_digests` 重建为全局缓存，主键由内容哈希、prompt 哈希、provider 和 model 组成，不再外键绑定 run。旧缓存数据随迁移保留；读取会刷新命中次数与最后访问时间，维护时执行 30 天 TTL 和 4096 条 LRU 上限。路由或模型变化不会误命中旧结果。

## 2. 章节内与增量检查点

章节 writer 首次产出后立即保存草稿，随后每轮验证和修订都原子保存 Markdown、attempt 与 revision 计数。取消、超时或修订失败时保留最后一版草稿；恢复时从该草稿继续，而不是重新生成整章。

增量 digest 路径也在每个 Chunk 成功后写入全局缓存并刷新运行检查点，因此中断只会重做尚未成功的部分。

## 3. DAG 节点恢复

恢复时读取 `note_pipeline_nodes` 的持久化状态，并与本次编译节点的 `input_hash` 对照。只有输入完全匹配的节点才恢复；输入变化的节点保持重新执行状态，避免复用过期产物。

## 4. 延后项

- P3-7：`note_pipeline_outputs` 和遗留节点 lease 列将在 P2-16 至少发布一个正式版本后，通过 v19 或更高版本删除；当前 v18 不做不可逆清理。
- P3-8：当前搜索仍是 SQLite `LIKE` 路径，没有确认到 FTS5 前置依赖，因此不增加无消费者的索引和迁移成本。

## 5. 验证

- 相同内容跨位置生成相同 Chunk ID；
- 新 run 可命中旧 run 的 digest，删除来源 run 后缓存仍存在，provider/model 不同则不命中；
- 章节中断后草稿跨数据库重开仍可恢复；
- DAG 节点仅在 `input_hash` 匹配时恢复；
- Rust 全量测试 427 passed，前端 226 passed，TypeScript 与 Rust 全目标检查通过。
