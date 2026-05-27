# 串口命令执行与输出捕获

## 背景

现有 `serial.write` 只负责向串口写入输入，调用方如果要查看命令响应，需要再去查 `serial.tail` 的事件流并手工截取对应片段。对 AI 调用、自动化排障和一次性诊断命令来说，这个流程不够稳定，也不够容易复用。

典型命令包括：

- `ip route`
- `iwpriv ra0 show psinfo`
- `cat /proc/net/nf_conntrack`

这些命令更适合以 request/response 方式执行：提交命令、等待 prompt 或 marker、返回本次命令捕获到的输出。

## 目标

- 增加 `SerialExecRequest` / `SerialExecResult` 共享模型，表达命令、超时和可选 `until` marker。
- 增加同步 API：`POST /api/devices/{device_id}/serial/exec`，直接返回 captured output，避免调用方再人工读取 tail。
- 增加异步 job action：`POST /api/devices/{device_id}/actions/serial.exec`，让需要审计和长期留痕的调用仍可走 job/evidence 模型。
- 复用同一个 server-level 串口命令执行能力，避免同步 API 和 job action 各自实现一套串口协议逻辑。
- 为 job action 持久化 `output.txt` 和 `result.json`，并把捕获输出发布到 job events。
- 更新 Device Hub operator skill，让一条命令取输出的默认路径变成同步 `/serial/exec`。

## 非目标

- 不改变 `serial.write` 的语义；它仍然只表示短生命周期的写入动作。
- 不把现有 `/actions/{action}` 接口改成同步等待接口；该接口继续表示启动 job。
- 不要求调用方必须启动 `serial.tail` 才能执行一次性串口命令。

## 验证

- API 集成测试使用 server 级串口 opener seam 和 `SerialStream::pair()` 模拟串口设备。
- 覆盖同步 `/serial/exec` 返回 output、matched marker 和 timeout 状态。
- 覆盖 `actions/serial.exec` job 持久化 `output.txt` / `result.json` 并发布串口输出事件。
