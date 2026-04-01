为了规范后续开发，本文将该模式定义为标准的“端口与适配器架构”（Ports and Adapters），也称“六边形架构”（Hexagonal Architecture）。

本文聚焦于标准定义，并区分“架构必选原则”与“项目可选治理规则”。

---

### 一、核心原则（The Golden Rules）

1. **依赖倒置（DIP）**：业务核心（Core）只依赖抽象（Ports），不依赖具体实现（Adapters）。
2. **边界清晰**：所有跨边界交互通过端口契约进行，端口定义“需要什么”，适配器定义“如何实现”。
3. **可替换性**：在不修改 Core 的前提下，可以替换 Adapter 实现。
4. **契约优先（Contract-First）**：先定义端口契约，再实现适配器。

---

### 二、组件角色定义

#### 1) Core（Domain/Application）
- **职责**：承载业务规则、流程编排、状态机、用例逻辑。
- **约束**：不直接依赖数据库、网络、消息系统、SDK 或具体框架。

#### 2) Ports（边界接口）
- **职责**：定义 Core 与外部系统交互的抽象契约。
- **类型**：
  - **Inbound Ports（输入端口）**：外部如何调用 Core（如 Use Case 接口）。
  - **Outbound Ports（输出端口）**：Core 需要的外部能力（如存储、网络、设备能力）。

#### 3) Adapters（适配器）
- **职责**：实现 Ports，完成协议/模型/调用方式转换。
- **类型**：
  - **Driving / Primary Adapters（入站适配器）**：将外部请求转换为 Inbound Port 调用（如 HTTP Controller、CLI、消息订阅者）。
  - **Driven / Secondary Adapters（出站适配器）**：实现 Outbound Port，调用外部系统（如 DB、SDK、第三方 API）。

#### 4) Composition Root（装配根）
- **职责**：在启动阶段完成依赖注入与装配（将具体 Adapter 绑定到 Port）。
- **约束**：装配逻辑集中，避免在业务代码中分散创建具体实现。

---

### 三、通信规范

1. **入站路径**：外部系统 -> Driving Adapter -> Inbound Port -> Core。
2. **出站路径**：Core -> Outbound Port -> Driven Adapter -> 外部系统。
3. **依赖方向**：源码依赖应始终指向抽象和内层，不应由 Core 反向依赖外层实现。
4. **模块间调用**：
   - 架构层面不强制“必须经中心转发”。
   - 若项目选择总线/中转模式，应明确为治理策略，而非 P&A 的定义要求。

---

### 四、检查清单（区分必选与可选）

| 类别 | 规范 | 说明 |
| :--- | :--- | :--- |
| **架构必选** | Core 不依赖具体基础设施实现 | 保持可测试与可替换 |
| **架构必选** | 跨边界调用通过 Ports 定义 | 保证边界稳定 |
| **架构必选** | 依赖注入在 Composition Root 完成 | 避免业务层硬编码实现 |
| **架构必选** | 端口契约版本与兼容策略明确 | 降低演进风险 |
| **项目可选** | 统一 DTO | 便于跨模块协作，但不是 P&A 必要条件 |
| **项目可选** | 统一错误码 | 便于观测与治理，但不是 P&A 必要条件 |
| **项目可选** | 禁止横向直接引用 | 可提升治理强度，但非通用硬约束 |
| **项目可选** | 核心统一调度/消息总线 | 属于实现选择，不是架构定义 |

---

### 五、何时使用此模式

- 存在多个外部实现且需要可替换（多供应商、多驱动、多平台）。
- 业务核心需长期稳定演进，避免被基础设施细节侵入。
- 需要高可测试性（Core 可用测试替身验证）。

不建议为了“看起来解耦”而新增纯透传层；没有真实替换需求或转换逻辑时，额外抽象会增加维护成本。

---

### 六、架构示意图（文字版）

```text
          [ Driving Adapter A ]     [ Driving Adapter B ]
                    \                       /
                     \                     /
                     [   Inbound Ports    ]
                              |
                           [ Core ]
                              |
                     [   Outbound Ports   ]
                     /                     \
                    /                       \
         [ Driven Adapter X ]      [ Driven Adapter Y ]

注：源码依赖方向应从外向内指向 Core/Ports 抽象。
```

**总结：**
Ports and Adapters 的核心是“**内核稳定、边界清晰、实现可替换**”。
项目可以叠加自己的治理规则（如总线中转、DTO 统一、错误码统一），但应明确这些是工程策略，而不是该架构本身的定义。

---

### 七、当前项目落地说明（device-hub）

- `server/src/api` 属于 Driving Adapter，负责把 HTTP 和 WebSocket 请求翻译成应用调用。
- `server/src/use_cases.rs` 表达应用用例，如 `start_action`、`cancel_job`、`rerun_job`。
- `server/src/services/job_manager.rs` 属于 Core 内的流程编排，负责任务生命周期、锁和超时，不直接依赖具体 SSH/Serial/J-Link 实现。
- `server/src/ports.rs` 定义 Core 需要的出站能力抽象，例如动作执行端口。
- `server/src/services/adapter_action_runner.rs` 是 Driven Adapter，实现动作执行端口并调用具体 adapter crate。
- `server/src/app.rs` 是 Composition Root，负责把 concrete adapter 装配到端口。

当前项目的治理约束：

- API 层不直接承担业务规则和设备执行细节。
- Core 层不直接 import `device-hub-adapter-*` crate。
- 测试优先通过 `server` 提供的 seam 注入替身，而不是直接依赖具体 adapter crate。
