# ADB Logcat 支持

## 背景

Device Hub 已经支持 SSH、serial 和 J-Link，但 Android 设备的日志仍需要操作者直接运行 `adb logcat`。这会让 Android 调试日志脱离统一 job 生命周期、事件缓存、artifact 持久化和 Web UI 日志视图。

本次变更把 ADB logcat 纳入 Device Hub 的一等能力，让 Android 设备可以通过设备注册表配置，并通过统一 action/job 模型查看日志。

## 用户故事

作为设备调试操作者，我希望能在 Web UI 中注册一个 ADB 设备，并启动 `adb.logcat` 实时查看 Android 日志，这样我可以在同一个 Device Hub 页面中观察 SSH、serial、J-Link 和 ADB 相关事件，而不需要另开本地终端运行 adb。

## 范围

- 在 shared schema 中增加可选 `adb` 配置和 `adb` 日志来源。
- 增加 `device-hub-adapter-adb`，通过 `adb logcat` 流式读取 Android 日志。
- 在 server use case 和 job runner 中支持 `adb.logcat`，包括 active tail 状态、reuse、取消和无超时策略。
- 在设备管理页面支持 ADB-only 设备注册和编辑。
- 在设备详情页提供 ADB actions，允许启动、停止和打开 `adb.logcat` job。
- 在 log viewer 中把 ADB 作为可过滤来源，并修正 xterm 换行渲染，使 logcat 每行从列首显示。

## 非范围

- 暂不实现 `adb shell`、`adb pull`、`adb push`、`adb reboot`。
- 暂不做 ADB 设备自动扫描；当前由操作者填写 `adb devices` 中显示的 serial。
- 暂不支持复杂 shell quoting 的 logcat 参数结构化编辑；当前使用空白分割参数，如 `-v time`。

## 验收标准

- 只配置 ADB 的设备可以通过 API 和 Web UI 注册成功。
- 注册后的 ADB 设备详情页显示 ADB actions。
- 启动 `adb.logcat` 后，日志以 `LogSource::Adb` 进入统一事件流和 job evidence。
- `reuse=true` 能返回已运行的 `adb.logcat` job。
- 取消 `adb.logcat` 能停止运行中的 adb 进程并更新 job 状态。
- xterm 日志视图按行正确渲染 logcat 输出。
