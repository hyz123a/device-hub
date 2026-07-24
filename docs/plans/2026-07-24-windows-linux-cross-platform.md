# Windows/Linux 原生跨平台支持 TDD 计划

## 背景

Device Hub 当前主要在 Linux/WSL 中开发和运行。通过 `usbipd-win` 把 Windows USB 串口附加到 WSL 虽然能够工作，但 USB/IP 连接重置后，Linux 会移除并重新枚举 tty，现有 `serial.tail` 仍持有旧句柄，无法继续采集日志。

项目的核心依赖 `tokio`、`axum`、`tokio-serial` 和 Rust 标准文件 API，本身具备跨平台基础。真正的平台差异集中在外部程序名称、主机路径、测试替身、构建脚本和硬件恢复语义。因此，本计划选择让现有 Device Hub Server 成为 Windows/Linux 原生程序，而不是立即引入另一套 Windows Agent 和远程协调协议。

## 用户故事

作为设备调试操作者，我希望在 Windows 或 Linux 主机上运行同一个 Device Hub，并直接访问该主机连接的串口、J-Link、ADB 和 SSH 工具，从而绕开 WSL USB/IP，持续采集设备日志，同时保持相同的 Web UI、HTTP API、job lifecycle 和 evidence 格式。

## 关键决策

- 维护一套 shared/server/use-case 代码，不按操作系统复制业务实现。
- Windows 适配开始前，必须先把固定持有全部 concrete adapter 的 `AdapterActionRunner` 重构为 capability registry；这是后续阶段的前置门禁。
- 每个 adapter crate 通过 Cargo feature 独立选择，编译进二进制的 adapter 还必须通过运行时配置显式注册；未启用的 adapter 不初始化、不探测外部工具、不暴露 action。
- 首轮采用“静态插件”模型：独立 adapter crate、可选依赖、运行时注册表。暂不使用 Rust DLL/SO 动态加载；未来确有第三方扩展需求时优先设计进程隔离的稳定协议。
- 平台差异只存在于 concrete adapter、composition root、构建和部署层。
- Windows 主发布目标为 `x86_64-pc-windows-msvc`。
- Linux 开发机使用 `cargo-xwin` 做 Windows MSVC 交叉检查和开发构建。
- Windows 原生 CI 负责最终编译、测试和发布验证；交叉编译不替代 Windows 实机测试。
- 串口配置保留现有 `port` 字段以兼容旧数据，并逐步增加稳定 USB 身份选择器。
- 意外 EOF 或可恢复的串口 I/O 错误表示断线，不表示 `serial.tail` 成功完成。
- 本阶段不引入远程 Coordinator/Node 协议；未来需要分布式实验室时，再从现有 adapter runtime 抽取 node。

## 架构边界

### 保持平台无关

- `server/src/api`
- `server/src/use_cases.rs`
- `server/src/services/job_manager.rs`
- shared API/domain contracts
- job state machine、locking、event filtering、evidence contracts

这些模块不能直接依赖 Windows/Linux API，也不能根据操作系统选择业务规则。

### 允许承载平台差异

- `adapters/ssh`
- `adapters/serial`
- `adapters/jlink`
- `adapters/adb`
- `server/src/app.rs`
- `server/src/services/adapter_action_runner.rs` 及按 adapter 拆分的 driven handlers
- `tools/`、CI 和 packaging

优先使用 adapter 内的小型纯函数或私有 platform helper，不创建覆盖整个应用的通用 `Platform` 抽象。

### Capability registry 边界

- `server` 提供稳定的 action/capability 注册 seam；registry 只按 action ID 路由到实现 `ActionRunner` port 的 handler，不包含 SSH、J-Link、串口或 ADB 业务分支。
- `server/src/app.rs` 是 composition root，根据编译期 feature 和运行时配置构造、注册 concrete adapter handler。
- 每个 handler 只依赖自己对应的 adapter crate；增加或移除一个 adapter 不得修改 job lifecycle orchestration。
- API 和 UI 从 capability snapshot 得知当前实例可用的 action，不能根据操作系统名称猜测能力。
- 已编译但运行时禁用的 adapter 与未编译的 adapter 都不注册 action；API 对不可用 action 返回稳定、可诊断的 capability error。

## 目标运行矩阵

| 主机 | Rust target | 推荐发行 feature | 串口示例 | 可选外部工具 |
| --- | --- | --- | --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | 按部署选择，至少 `serial` | `/dev/ttyUSB0`、`/dev/serial/by-id/...` | `ssh`、`adb`、`JLinkExe` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | 默认 `serial`，其余按需 | `COM25` | `ssh.exe`、`adb.exe`、`JLink.exe` |

macOS、Windows ARM64 和 Linux ARM64 不作为本轮验收目标，但新增设计不得无必要地阻止后续扩展。

## TDD 实施阶段

### 阶段 1：把固定 AdapterActionRunner 重构为 capability registry

本阶段是 Windows 平台适配的强制前置门禁。阶段未完成时，不开始 host 平台分支、Windows 串口或 Windows packaging 实现。

先增加失败测试：

- 仅启用 `serial` feature 时，Server 能构建和启动，且不要求 SSH、J-Link、ADB adapter。
- 不启用某 adapter 时，对应 action 不存在于 capability snapshot，且调用返回稳定的 unavailable error。
- adapter 已编译但被运行时配置禁用时，不构造 concrete adapter、不探测外部程序、不注册 action。
- registry 能把 action ID 路由到注入的 server-level fake handler，并保持 start、cancel、rerun、事件和 artifact 语义。
- 未注册或重复注册 action 时产生确定、可测试的错误。

实现内容：

- 将 `device-hub-adapter-serial`、`ssh`、`jlink`、`adb` 改为 `server/Cargo.toml` 中的 optional dependencies，并分别提供 Cargo features。
- 提供明确的 feature profile，例如 `serial`、`serial,ssh` 和 `full`；Windows 发布不默认携带 SSH/J-Link/ADB。
- 用 action/capability registry 取代固定包含四个 concrete adapter 字段和集中 `match` 的 `AdapterActionRunner`。
- 将各 adapter action execution 拆到 driven-adapter handler；handler 负责把 adapter 输出转换成现有 job events/evidence，不把 concrete 类型泄漏到 use case 或 job manager。
- composition root 根据 feature 编译对应注册代码，再根据运行时配置决定是否实例化和注册。
- 提供 capability snapshot/API，至少返回 adapter ID、action IDs、enabled/available 状态和不可用原因；前端据此隐藏或禁用功能。
- 保持现有公开 action 名称、job 状态、锁、取消、rerun、artifact 和 evidence 合同不变。

建议 feature 形态：

```toml
[features]
default = ["serial"]
serial = ["dep:device-hub-adapter-serial"]
ssh = ["dep:device-hub-adapter-ssh"]
jlink = ["dep:device-hub-adapter-jlink"]
adb = ["dep:device-hub-adapter-adb"]
full = ["serial", "ssh", "jlink", "adb"]
```

验收：

```bash
cargo test --workspace
cargo test -p device-hub-server --no-default-features --features serial
cargo check -p device-hub-server --no-default-features --features serial,ssh
cargo check -p device-hub-server --all-features
```

- `server/src/services/job_manager.rs` 仍只依赖 ports。
- `server/src/api` 不包含 concrete adapter dispatch。
- serial-only 构建不会链接其他 adapter，也不会要求安装其外部工具。
- 新增 adapter 可以通过实现 handler 并在 composition root 注册完成，不需要修改 job lifecycle。

### 阶段 2：建立跨平台编译护栏

先增加会失败的 Windows target 编译检查，暴露生产代码和测试代码中的 Unix-only 依赖。

实现内容：

- 为开发环境记录 `clang`、`lld`、`llvm`、Windows MSVC target 和 `cargo-xwin` 前置条件。
- 增加统一的 Windows Server 构建命令，只构建 `device-hub-server` 及其依赖，不把 Yew frontend crate 当作 Windows target 编译。
- Linux 继续构建 WASM frontend bundle，再通过 `DEVICE_HUB_FRONTEND_BUNDLE` 嵌入 Windows Server。
- 增加 CI 矩阵的最小骨架：Linux native test、Linux-to-Windows cross check、Windows native test/build。

验证：

```bash
cargo test --workspace

DEVICE_HUB_FRONTEND_BUNDLE="$PWD/target/frontend-bundle/frontend.bundle.tar" \
  cargo xwin check \
  -p device-hub-server \
  --no-default-features \
  --features serial \
  --target x86_64-pc-windows-msvc
```

只有阶段 1 的 capability registry 验收通过后，才允许执行本阶段及后续 Windows 工作。

### 阶段 3：收敛 host 平台差异

先用 adapter 单元测试固定每个平台应产生的命令和路径参数，再修改实现。

实现内容：

- J-Link 默认程序：Windows 使用 `JLink.exe`，Linux 使用 `JLinkExe`。
- J-Link command file 正确引用包含空格或 Windows 分隔符的 firmware path。
- SSH home 展开同时支持 Windows 用户目录和 Linux home。
- SSH known-hosts sink 在 Windows 使用兼容的 null device，在 Linux 使用 `/dev/null`。
- `ssh_program`、`jlink_program`、`adb_program` 保持显式配置优先于平台默认值。
- 明确 SSH 的 `tail -F` 和 reboot shell 是远端设备命令，不随 Device Hub 主机平台切换为 PowerShell。
- 前端串口输入提示同时展示 COM 和 tty 示例。

验收：

- 平台默认值只出现在 adapter/private helper。
- use case、job manager 和 API 中没有新增 `cfg(windows)`/`cfg(unix)`。
- Linux 现有配置保持兼容。

### 阶段 4：替换 Unix-only 测试替身

先让 Windows target 的 test compilation 失败，再逐个把外部行为迁移到稳定 seam。

实现内容：

- Server API 集成测试通过 server-level port/seam 注入 action、host discovery 和串口行为。
- Server 集成测试不直接创建 Unix executable shell scripts，也不直接依赖 concrete adapter crates。
- Adapter 自身的进程测试使用可在 Windows/Linux 执行的 Rust fixture binary，或使用 adapter 内部可注入的 process boundary。
- 只有真正依赖 PTY/Unix kernel primitive 的测试保留 `#[cfg(unix)]`，并在 Windows 增加等价的行为测试或显式说明覆盖差异。
- 删除测试模块顶层无条件使用的 `std::os::unix::*`。

验收：

```bash
cargo test --workspace
cargo xwin test -p device-hub-server --target x86_64-pc-windows-msvc --no-run
```

第二条只验证交叉编译；测试执行结果以 Windows native CI 为准。

### 阶段 5：Windows 原生串口发现与稳定身份

先通过 server-level host discovery seam 固定 API 合同，再扩展 adapter 枚举信息。

建议的向后兼容模型：

```rust
pub struct SerialDeviceSelector {
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub serial_number: Option<String>,
    pub physical_location: Option<String>,
}

pub struct SerialConfig {
    pub port: String,
    pub baud: u32,
    pub selector: Option<SerialDeviceSelector>,
}
```

实现内容：

- Windows 枚举结果返回 `COMx`，Linux 返回 tty path。
- 在底层库可提供时，发现结果同时返回 VID、PID、serial number 和物理位置。
- 配置了 selector 时，启动 job 前解析当前 endpoint；`port` 作为兼容 fallback 和最近一次观察值。
- 多个相同 VID/PID 的 CH340 必须使用 serial number 或物理位置消除歧义。
- UI 展示稳定身份和当前 endpoint，而不把 endpoint 当永久设备 ID。

验收：

- Windows 可以直接创建并启动 `COM25` 的 `serial.tail`。
- Linux 现有 `/dev/ttyUSB0` 配置继续可用。
- 端口号变化后 selector 可以解析到新的 endpoint。
- selector 匹配零个或多个设备时返回可诊断错误。

### 阶段 6：串口断线重连

先增加服务层测试，覆盖断开、重新发现、继续输出、取消和重试耗尽。

期望状态流：

```text
connected
  -> disconnected
  -> waiting_for_device
  -> reconnecting
  -> connected
```

实现内容：

- `serial.tail` 遇到意外 EOF、`BrokenPipe`、`ConnectionReset`、`NotFound` 或平台等价错误时进入重连，而不是调用 `succeed_job`。
- 通过 serial adapter/port boundary 重新发现和打开设备；job manager 不感知具体串口实现。
- 使用有上限的指数退避，并发布 system events，记录尝试次数、旧 endpoint 和新 endpoint。
- Job 被取消时立即终止等待、释放 session registry 和串口句柄。
- 明确重连期间存在不可恢复的日志窗口；event/evidence 中记录断线区间。
- 若使用固定 `port` 且没有 selector，只允许重试同一路径，不猜测另一个相同型号设备。

验收：

- 短暂断开并重新出现后，原 `serial.tail` job 继续运行并收到新日志。
- 重连不会创建第二个 job，也不会绕过现有设备锁和 active-tail 语义。
- 取消等待中的 job 最终进入 `cancelled`。
- 非预期 EOF 不再被记录为 `serial.tail completed`。

### 阶段 7：构建、打包和实机验证

实现内容：

- 增加 PowerShell 构建入口，或引入跨平台 `xtask` 统一 frontend bundle、server build 和 package。
- Windows package 包含 Server、示例配置和启动说明；第一阶段不强制制作 MSI。
- 增加 Windows Service 部署说明或独立后续任务。
- Windows native CI 构建 MSVC release artifact。
- serial-only Windows release 在带 CH340 的环境执行必须的硬件 smoke tests。
- SSH、J-Link、ADB 使用独立 opt-in release profile 和对应环境执行 adapter-specific smoke tests，不作为 serial-only 发布的安装或验收前提。

Windows 实机 smoke checklist：

- Server 启动并提供嵌入式 Web UI。
- 创建、更新、删除设备后 registry 能持久化。
- 枚举多个 COM 端口。
- `serial.tail`、`serial.write`、`serial.exec` 正常。
- CH340 拔插后原 tail 可恢复。
- 启用 `jlink` profile 时，`JLink.exe` probe/reset/flash 参数正确。
- 启用 `adb` profile 时，`adb.exe logcat` 可启动、取消和 reuse。
- 启用 `ssh` profile 时，Windows OpenSSH 可以使用显式密钥路径执行远端命令。
- job events、raw stream、artifacts 和 evidence 可以写入 Windows 路径。

## 非目标

- 本轮不实现云端控制面或远程 Device Hub Node。
- 不承诺串口物理断开期间零日志丢失；零丢失需要设备端缓存或本地 durable spool/补传协议。
- 不在首个 Windows 版本中实现 GUI tray、自动更新器或 MSI。
- 不改变现有公开 API action 名称和 job 状态枚举。
- 不把 SSH 远端命令泛化为 Windows target shell；当前受控设备仍按现有 Linux SSH 语义运行。

## 风险与控制

- **交叉编译通过但 Windows 运行失败**：以 Windows native CI 和实机 smoke test 为准。
- **外部 CLI 参数跨平台不一致**：为 command construction 写 adapter 测试，并在实机验证真实工具版本。
- **相同 CH340 无稳定 serial number**：允许配置物理位置，并在歧义时失败而不是猜测。
- **无限重连掩盖故障**：重连事件、退避状态和累计断线时间必须可观察；策略参数需要有安全上限。
- **Windows 文件占用语义不同**：覆盖 registry、artifact append/read、取消和进程 teardown 测试。
- **平台判断扩散**：review 时拒绝在 API、use case 和 job manager 中引入 host OS 分支。
- **静态 registry 再次退化为集中 match**：每个 adapter 使用独立 handler，registry 只负责 action lookup 和 dispatch，并用新增 adapter 的架构测试约束修改范围。
- **编译 feature 与运行配置不一致**：capability snapshot 明确区分 not compiled、disabled 和 unavailable，启动日志记录最终注册结果。

## 完成标准

- 固定持有全部 adapter 的 `AdapterActionRunner` 已被 capability registry 替代，且 registry 阶段在所有 Windows 专项改动之前独立验收。
- serial、SSH、J-Link 和 ADB 都是独立 Cargo features，并可在运行时禁用；未启用 adapter 不初始化、不探测工具、不注册 action、不出现在可用 UI 操作中。
- serial-only Linux/Windows 构建不链接 SSH、J-Link 或 ADB adapter，也不要求安装对应外部工具。
- capability API 能可靠说明 compiled、enabled、available 和 unavailable reason。
- 同一份设备/API/job 模型可以在 Linux 和 Windows 使用。
- Linux 完整测试保持通过。
- Windows MSVC native CI 的 check、test 和 release build 通过。
- Linux `cargo-xwin` 可以生成带嵌入前端的 Windows Server。
- Windows Server 直接使用 COM 口，不依赖 WSL 或 usbipd。
- Windows serial-only 实机上的串口断线重连和 evidence smoke test 通过；SSH、J-Link、ADB 仅在各自 opt-in profile 中验收。
- 文档明确区分交叉编译验证、Windows native 验证和硬件实机验证。

## 实施状态（2026-07-24）

| 阶段 | 状态 | 可复核证据 |
| --- | --- | --- |
| Capability registry 与可选 adapter | 已实现 | serial-only/default/full 构建；`/api/capabilities`；最终注册状态启动日志；runtime disable 与注入 handler 的 start/cancel/rerun/events/artifact/evidence 测试 |
| Windows 编译与平台差异 | 已实现 | `tools/check-windows.sh`；Windows MSVC serial/full check 与 test no-run；交叉生成的测试 EXE 在 Windows 上执行通过 |
| 跨平台测试 seam | 已实现 | server-level action/host-discovery/serial 注入；SSH/J-Link/ADB 使用 Windows/Linux 共用的 Rust process fixtures |
| COM 枚举与稳定身份 | 已实现并完成 Windows 真机验证 | 两只同序列号 CH340 返回不同 SetupAPI location paths；用 `COM404` + location selector 成功解析并打开 COM24；active-tail API 报告实际 `current_endpoint: COM24` |
| 串口断线重连 | 已实现并完成 Windows 真机验收 | CH340 物理拔插后同一 job 从 COM24 恢复到 COM24，断线期间清除失效 current endpoint，记录 outage，并收到重连后的 stdout；另有 EOF、有限重试耗尽和无限等待取消测试 |
| 构建、打包与 CI | 已实现 | Linux、Windows native、cargo-xwin workflow；PowerShell 与 Linux 打包入口 |
| Windows CH340 实机 smoke | 已完成 | 原生 EXE、最新嵌入 UI、capability 启动日志、registry CRUD/重启持久化、等待中取消、Windows artifact/evidence、COM24/COM25/COM47 枚举、sleep inhibitor、stale endpoint + location selector、锁屏观察、物理拔插重连、重连后 stdout/raw-stream、HomeBase target-safe `serial.write`/`serial.exec` 均已通过 |

当前自动化验收命令：

```bash
cargo test --workspace --all-features
cargo test --workspace
cargo test -p device-hub-server --no-default-features --features serial
cargo check -p device-hub-server --no-default-features --features serial,ssh
cargo check -p device-hub-server --all-features
cargo clippy --workspace --all-features --all-targets -- -D warnings
tools/check-windows.sh
```

Windows 原生运行报告由 `tools/windows-runtime-smoke.ps1` 生成。无硬件模式覆盖 capability、嵌入 UI、registry CRUD/重启持久化、缺失 COM 的等待/取消和 Windows artifact/evidence；传入 CH340 selector 后额外覆盖真实 COM 打开、实际 endpoint 与 sleep inhibitor。必须人工完成的最终报告由 `tools/windows-serial-smoke.ps1` 生成。

### Windows CH340 实机验收记录

验收主机为 `HYZ`，Windows `10.0.26100.0`，PowerShell `5.1.26100.4202`。目标 selector 为 `PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(2)#USB(4)#USB(4)`，配置 fallback 故意使用不存在的 `COM404`。

| 报告 | Job | 结果 |
| --- | --- | --- |
| `target/windows-serial-smoke-lock.json` | `job_1a4a6835e0644d098d485a2ef8dafd32` | 锁屏观察通过；检测到 COM24 物理断开；报告因首次重连后没有 stdout 而按预期失败，没有把仅打开端口误判为完整通过 |
| `target/windows-serial-smoke-reconnect.json` | `job_29e1cbed39d746418f30edf673843551` | 通过；COM24 -> disconnected/null endpoint -> COM24；`outage_ms=29366`；同一 job 收到重连后的 stdout，正常取消，evidence 含 `event-summary.json`、`events.ndjson`、`job.json` 和 `raw-stream.log` |
| Windows API `serial.write` | `job_afe6629e1e5141dab81f8824b0ed6306` | 通过；Windows 原生 Server 经 COM25 向 Linux HomeBase 发送只读 `printf` + `uname -srm`，write job 成功，活动 tail 捕获唯一标记 `DEVICE_HUB_WINDOWS_WRITE_OK` |
| Windows API `serial.exec` | `job_034862beb4c54b52866d21fffc6290fb` | 通过；异步 command/expect 返回 `Linux 6.6.110 mips`，匹配 `DEVICE_HUB_WINDOWS_ASYNC_EXEC_DONE` 且未超时，evidence 包含 `output.txt` 和 `result.json`；同步 `/serial/exec` 使用独立标记同样通过 |

原始报告 SHA-256：

- lock：`9ea95a5973ca315cef009a6137309c4839db58c3d44fea0110f2e77f934d63a5`
- reconnect：`1e6ea68ba69d454539d8b9178a3f3ec5a1f30b5b72c19deba92d3d7ec6580afc`
