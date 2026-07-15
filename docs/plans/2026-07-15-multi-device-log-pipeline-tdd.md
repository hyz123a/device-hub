# 多设备日志管线性能优化 TDD 指导

## 背景

Device Hub 同时连接多个高日志量设备时，日志缓存达到上限后会出现明显卡顿。现有实现中，服务端 5,000 条内存事件缓存使用 `VecDeque`，滚动淘汰本身是 O(1)；主要风险集中在以下路径：

- 前端底层事件存储最多保留 1,000,000 条，但 UI 选择的 buffered cap 只限制可见快照。
- 前端定时复制、过滤并比较完整事件窗口，窗口滚动后仍会产生与缓存规模相关的工作量。
- dedup hash 不随事件淘汰，长时间运行时状态持续增长。
- `tail=N` 在完整反序列化 `events.ndjson` 后才截取尾部。
- job evidence 会读取完整事件历史，Job 详情页还会再次请求事件。
- artifact 持久化对每条事件同步打开并写入多个文件。

这次开发在同一分支内完成，但必须按阶段推进 TDD。任何阶段都不得以“后续阶段会修好”为理由留下失败测试。

## 用户故事

作为同时调试多个设备的操作者，我希望实时日志在缓存滚动后仍保持流畅，打开大型历史 Job 时不会阻塞服务，并且所有日志和 evidence 仍能按顺序、完整、可追溯地持久化。

## 目标

- 前端实时日志内存和单次更新工作量保持有界。
- xterm 正常流式更新只消费新增 batch，不重复处理完整历史窗口。
- `GET /api/jobs/{job_id}/events?tail=N` 的读取成本由尾部数据量决定，而不是由整个 artifact 大小决定。
- evidence 优先使用写入阶段维护的有界摘要；旧 artifact 仅使用有界事件样本回退，且保持现有 API 合同和诊断价值。
- artifact 写入复用打开的文件并批量缓冲，同时保持顺序、完整性和错误可观察性。
- 建立可重复的多设备日志负载验证，防止性能退化再次进入主分支。

## 非目标

- 不改变 `LogEvent` JSON schema。
- 不删除完整 `events.ndjson` 或 `raw-stream.log` artifact。
- 不改变无 `tail` 参数时下载/查询完整 Job 事件历史的现有合同。
- 不引入 Elasticsearch、数据库或远程日志服务。
- 不用静默丢日志来换取前端或持久化吞吐。
- 不在本次重构中改变设备 action、job 状态机或锁语义。

## 架构约束

- `server/src/api` 只解析请求并调用 use case，不新增文件读取、tail 截取或 evidence 业务规则。
- Job 事件查询和 evidence 组装进入 `server/src/use_cases.rs` 或其明确委托的应用服务。
- 文件系统 tail reader 和 buffered artifact writer 是 driven adapter，通过 `server` 端口注入；核心生命周期代码不得依赖具体 adapter crate。
- 集成测试通过 server seam/port 注入外部行为，不直接依赖 `device-hub-adapter-*`。
- 前端先抽取可独立测试的有界事件缓冲和增量 batch 模型，再接入 Yew hooks 与 xterm bridge。

## 全程测试门禁

每个阶段遵循 Red -> Green -> Refactor：

1. 先增加一个只描述目标行为的失败测试，并确认失败原因正确。
2. 编写最小实现使新增测试通过。
3. 运行该阶段相关测试和此前所有阶段测试。
4. 重构后再次运行相同测试，确保没有依赖偶然实现细节。
5. 阶段结束时保持工作树可构建、测试全绿，再进入下一阶段。

通用门禁命令：

```bash
cargo fmt --all --check
cargo test -p device-hub-frontend
cargo test -p device-hub-server
cargo test -p device-hub-server --test api
cargo clippy --workspace --all-targets -- -D warnings
```

涉及 xterm bridge 或浏览器行为时额外运行：

```bash
npm --prefix frontend run build:assets
npm --prefix frontend run test:e2e
```

## 阶段 0：固化基线与测试 seam

### Red

- 将当前只验证“小 Vec 截断”的测试替换为真实有界存储合同。
- 为持久化 reader/writer 增加可注入或可使用 `Read + Seek`、内存 sink 测试的稳定 seam。
- 记录一个可重复的多设备输入样本和基线测量方式，不把当前机器的绝对耗时写成通用正确性断言。

### Green

- 测试 seam 只提供注入和观测能力，不提前实现优化。
- 保持现有 HTTP、WebSocket、artifact 内容和 job evidence 合同不变。

### Refactor 门禁

- 现有前端单元测试、ArtifactStore 单元测试和 API 集成测试全部通过。
- 新 seam 位于 server 边界，不让 API 测试依赖具体设备 adapter。

## 阶段 1：前端有界缓存

### Red

至少增加以下行为测试：

- `bounded_event_buffer_evicts_oldest_events_at_capacity`
- `bounded_event_buffer_preserves_interleaved_device_order`
- `bounded_event_buffer_evicts_dedup_identity_with_event`
- `changing_buffer_cap_immediately_bounds_underlying_store`
- `duplicate_detection_remains_correct_after_rollover`
- `clear_resets_events_and_dedup_state`

测试必须跨越容量多轮写入，不能只使用 6 条缩到 3 条的单次样例。

### Green

- 使用 `VecDeque` 或分块环形结构承载事件。
- 每个保留事件与其 dedup identity 具有一致生命周期。
- UI buffered cap 直接决定底层保留容量；改变 cap 时立即淘汰超限旧事件。
- 明确定义增大 cap 的行为：只能保留变更后新到达的事件，不承诺恢复已淘汰历史。

### Refactor 门禁

- push 和单条淘汰不随已保留事件数线性增长。
- 缓冲区与 dedup 状态的大小都不超过可证明的上限。
- 搜索、来源过滤、分页和 Clear cache 的现有行为保持正确。

## 阶段 2：xterm 增量渲染

### Red

至少增加以下行为测试：

- `terminal_receives_only_new_batch_during_normal_append`
- `terminal_receives_only_new_batch_after_buffer_rollover`
- `terminal_preserves_interleaved_device_event_order`
- `terminal_resets_once_when_filter_or_stream_identity_changes`
- `terminal_does_not_replay_full_history_on_render_flush`
- `terminal_batch_normalizes_newlines_without_changing_messages`

浏览器层至少增加一个 Playwright 场景：持续注入多批 WebSocket 日志，验证终端内容顺序、窗口滚动后的新日志以及 UI 控件可响应。

### Green

- WebSocket 收到事件后先进入有界缓存，同时累积待渲染 batch。
- 32 ms flush 只向 xterm bridge 发送该周期新增且匹配当前 stream 的文本。
- 初次进入、重连、filter/search 改变等 stream identity 变化允许一次有界 snapshot reset；正常 append 和 rollover 不允许全量 reset。
- Event 视图可以从有界缓存派生分页结果，但不得迫使 Stream 视图复制完整事件集合。

### Refactor 门禁

- 删除基于完整 previous/current 事件 Vec 的最长重叠扫描。
- Yew props 不再在每次 flush 中携带完整日志窗口。
- xterm 的 10,000 行 scrollback 与应用事件缓存是两个独立且有界的概念。

## 阶段 3：有界 tail 读取与 evidence

### Red

至少增加以下存储/API 行为测试：

- `read_tail_events_returns_last_n_in_original_order`
- `read_tail_events_handles_final_line_with_and_without_newline`
- `read_tail_events_handles_n_zero_and_n_larger_than_history`
- `read_tail_events_does_not_parse_unneeded_old_prefix`
- `get_job_events_tail_uses_bounded_reader_port`
- `get_job_events_without_tail_preserves_full_history_contract`
- `job_evidence_uses_bounded_event_samples`
- `job_detail_does_not_request_the_same_event_history_twice`

`read_tail_events_does_not_parse_unneeded_old_prefix` 应通过 reader seam、计数 reader 或旧前缀中的无关损坏行证明读取边界，不能只用响应耗时作断言。

### Green

- 对 seekable NDJSON 文件从尾部按块读取，收集足够的完整行后按原顺序解析最后 N 条。
- `tail` 参数在存储读取前生效；禁止先构建完整 `Vec<LogEvent>` 再截断。
- writer 在 `flush_job` 屏障同步落盘固定大小的 `event-summary.json`（通道计数、outcome、last error）；evidence 优先读取该摘要和 artifact metadata，不重复读取事件历史。
- 兼容没有摘要的旧 artifact 时，evidence 只读取生成摘要所需的有界 tail/highlight 样本。
- Job 详情页分别获取轻量 evidence metadata 与一次有界事件历史，避免重复加载同一历史。

### Refactor 门禁

- API handler 不直接访问 `ArtifactStore`。
- tail reader 的最大临时缓冲有明确上限；遇到超大单条事件时有测试覆盖的确定行为。
- 完整 artifact 下载仍保持字节完整，现有客户端合同不变。

## 阶段 4：buffered artifact writer

### Red

至少增加以下行为测试：

- `artifact_writer_preserves_event_order_across_batches`
- `artifact_writer_writes_raw_stream_only_for_stdout_and_stderr`
- `artifact_writer_isolates_concurrent_jobs`
- `artifact_writer_flushes_before_job_completion_is_observable`
- `artifact_writer_flushes_and_closes_on_shutdown`
- `artifact_writer_applies_bounded_backpressure_without_dropping_events`
- `artifact_writer_surfaces_open_write_and_flush_errors`
- `cancelled_job_keeps_all_events_accepted_before_cancellation`

### Green

- 通过持久化端口把事件交给专用 writer 生命周期管理。
- 按 job 复用 `BufWriter<File>`，允许按事件数、字节数或短周期批量 flush。
- writer 队列必须有界；队列满时采用可观测的背压，不静默丢弃事件。
- job 完成、取消、失败以及 server shutdown 都必须提供 flush/close 屏障。
- writer 错误必须进入结构化错误路径，不能只 `eprintln!` 后继续宣称 evidence 已持久化。

### Refactor 门禁

- `EventBus` 不再承担具体文件打开和写入细节。
- job lifecycle 只依赖持久化端口，不依赖文件系统实现。
- 并发 job 之间不串写，单个 job 的 `events.ndjson` 与 `raw-stream.log` 顺序一致。

## 阶段 5：多设备负载与性能回归

### 正确性负载

- 同时模拟至少 4 个设备来源，事件交错写入并跨越前端 buffer 多轮。
- 验证每个设备和全局流的顺序、数量、dedup、过滤、取消及最终 artifact。
- 在负载过程中反复打开 Job 详情和执行 `tail=N`，确认不会影响实时订阅正确性。

### 性能观测

提供仓库内可重复运行的负载工具或 ignored test，至少输出：

- 输入事件数和字节数。
- 前端缓冲容量、dedup 条目数和每次 flush batch 大小。
- artifact writer 队列峰值和 flush 次数。
- `tail=N` 扫描字节数。
- Job 详情/evidence 响应耗时。
- server 峰值 RSS 或等价的进程内存观测。

正确性测试断言结构性上限，例如“条目数不超过 cap”“tail 扫描量与尾部窗口同阶”；机器相关的绝对延迟和 RSS 阈值放在单独性能基线中，避免 CI 抖动造成误报。

仓库内性能 profile 可重复运行：

```bash
cargo test -p device-hub-frontend frontend_multi_device_log_buffer_performance_profile -- --ignored --nocapture
cargo test -p device-hub-server multi_device_log_pipeline_performance_profile -- --ignored --nocapture
cargo test -p device-hub-server read_tail_events_scans_only_the_tail_window -- --nocapture
cargo test -p device-hub-server --test api large_job_detail_performance_profile -- --ignored --nocapture
```

这些 profile 分别输出前端输入事件/消息字节/cap/dedup/最大 batch、writer 事件数/artifact 字节/峰值 outstanding/flush、tail 扫描字节，以及大型 Job evidence + tail 的响应耗时与进程 RSS。输出用于同机版本对比；普通测试负责断言有界容量、完整性和顺序。

### 完成门禁

- 全量 workspace 测试、前端构建和日志相关 E2E 全部通过。
- 多设备负载跨越缓存上限后，不出现与完整保留历史规模相关的单条淘汰或正常渲染工作。
- 打开 GB 级历史 Job 时，不完整反序列化 `events.ndjson`。
- 长时间运行后，前端 event/dedup、writer queue 和 server live cache 都保持有界。
- 所有已接受日志在正常完成、失败和取消路径中均可从 artifact 恢复。

## 提交建议

保持一个阶段一个可审查提交，推荐顺序：

1. `test(logs): characterize bounded multi-device pipeline`
2. `perf(frontend): bound live event and dedup storage`
3. `perf(frontend): stream incremental xterm batches`
4. `perf(server): read persisted event tails incrementally`
5. `perf(server): bound evidence event sampling`
6. `perf(server): buffer artifact persistence`
7. `test(logs): add multi-device performance regression harness`

每个提交都应包含对应测试，不把测试集中推迟到最后一个提交。
