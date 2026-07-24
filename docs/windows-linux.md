# Windows/Linux builds

Device Hub uses one platform-neutral API, use-case, job lifecycle, event, and evidence model. Host integrations are optional static plugins selected by Cargo features and then enabled or disabled by runtime configuration.

## Adapter profiles

The default server profile contains only `serial`.

```bash
cargo build -p device-hub-server
cargo build -p device-hub-server --no-default-features --features serial,ssh
cargo build -p device-hub-server --no-default-features --features full
```

An adapter absent from the selected feature set is reported as `not_compiled`. An adapter included in the binary but set to `false` under `adapters` is reported as `disabled`. Neither state registers actions or probes external tools.

## Build Windows on Linux

Install the MSVC target and `cargo-xwin`. LLVM/LLD are required by environments that do not already provide a compatible linker.

```bash
sudo apt install clang lld llvm
rustup target add x86_64-pc-windows-msvc
cargo install --locked cargo-xwin
```

Run the Windows compile gates:

```bash
tools/check-windows.sh
```

Build the current Yew frontend and a serial-only Windows release:

```bash
bash tools/build-frontend-bundle.sh
tools/build-windows-from-linux.sh
```

The executable is written to:

```text
target/x86_64-pc-windows-msvc/release/device-hub-server.exe
```

Cross-compilation proves that the source and dependencies support the Windows MSVC target. Windows native CI and real-hardware smoke tests remain authoritative for runtime behavior.

The Windows-native test job also executes SSH/J-Link/ADB process behavior against Rust fixture executables. These fixtures validate command invocation, output/error handling, cancellation-facing streams, and J-Link command-file behavior without requiring the optional vendor tools to be installed.

## Run the Windows server from WSL

After cross-compiling, WSL can start the Windows-native server directly. The launcher copies the executable and selected configuration into `%LOCALAPPDATA%\DeviceHub`, so registry, artifacts, PID state, and logs remain on the Windows filesystem:

```bash
tools/run-windows-from-wsl.sh start --config ./devices.windows.yaml
tools/run-windows-from-wsl.sh status
tools/run-windows-from-wsl.sh logs --follow
tools/run-windows-from-wsl.sh stop
```

The launcher defaults to `127.0.0.1:3001`, leaving the Linux/WSL development server free to use its normal `127.0.0.1:3000`. Windows and WSL loopback forwarding can expose both environments through localhost, so they must not bind the same port. Startup verifies that the listening Windows PID is the process it launched and rejects a port already served by WSL or another process.

Use `foreground` instead of `start` to keep server output attached to the current terminal. `restart` accepts the same `--server`, `--config`, `--runtime-dir`, `--bind`, and `--timeout` options as `start`. The release package includes `devices.windows.yaml`; update its host-specific selectors before using it elsewhere. When that file is absent, the launcher falls back to `devices.example.yaml`.

The default background files are:

```text
%LOCALAPPDATA%\DeviceHub\device-hub.pid
%LOCALAPPDATA%\DeviceHub\device-hub-process.json
%LOCALAPPDATA%\DeviceHub\stdout.log
%LOCALAPPDATA%\DeviceHub\stderr.log
```

Both WSL paths and Windows paths are accepted. For example, an unpacked Windows package can be selected from WSL with `--server /mnt/c/Users/<user>/Desktop/device-hub-windows-serial/device-hub-server.exe`. This process accesses `COM` ports natively and does not require attaching the USB adapter to WSL.

After building, a Windows host can verify the native executable, embedded UI, serial-only capability profile, COM discovery API, registry CRUD/restart persistence, and Windows artifact paths without hardware:

```powershell
.\tools\windows-runtime-smoke.ps1 `
  -ServerPath .\target\x86_64-pc-windows-msvc\release\device-hub-server.exe
```

From the unpacked release package, use `.\windows-runtime-smoke.ps1 -ServerPath .\device-hub-server.exe`. Windows-native CI runs this check and uploads `windows-runtime-smoke.json`.

On a host with an available adapter, add `-SerialPort COM25` to open the real port, confirm the Windows sleep inhibitor event, observe the connected event and runtime endpoint, persist/download its evidence, and cancel the tail without writing to the device. To prove stable rediscovery independently of a stale endpoint, pass a nonexistent fallback port plus a location returned by `/api/host/serial-ports`:

```powershell
.\tools\windows-runtime-smoke.ps1 `
  -ServerPath .\target\x86_64-pc-windows-msvc\release\device-hub-server.exe `
  -SerialPort COM404 `
  -SelectorPhysicalLocation 'PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(2)#USB(4)#USB(4)'
```

## Build on Windows

Download or build `target/frontend-bundle/frontend.bundle.tar`, then run:

```powershell
rustup target add x86_64-pc-windows-msvc
.\tools\build-windows.ps1 -Release
```

The default Windows package is serial-only. SSH, J-Link, and ADB are opt-in features and do not need to be installed for this profile.

## Runtime configuration

```yaml
adapters:
  serial: true
  ssh: false
  jlink: false
  adb: false
  serial_reconnect:
    max_attempts: null
    initial_delay_ms: 100
    max_delay_ms: 2000
    prevent_system_sleep: true

devices:
  - id: windows-uart
    label: Windows UART
    serial:
      port: COM25
      baud: 115200
      selector:
        vid: 6790       # 0x1a86
        pid: 29987      # 0x7523
        serial_number: CH341-A
```

The endpoint (`COM25` or `/dev/ttyUSB0`) is the current observation. When a selector is present, Device Hub resolves the current endpoint from the USB identity before opening and reopening the port. On Windows, discovery enriches the serial library metadata with the SetupAPI location path, allowing otherwise identical adapters to be distinguished by their physical USB topology. A selector matching zero or multiple devices fails diagnostically instead of guessing.

`serial.tail` treats unexpected EOF and recoverable open/read errors as disconnects. It emits disconnect, waiting, reconnecting, and reconnected events, re-resolves the selector, and keeps the same job running. A successful reconnect records both `outage_ms` and cumulative `total_disconnected_ms` so the evidence exposes the interval in which host-side log capture was impossible. Retry waits are capped; `max_attempts: null` keeps monitoring until cancellation, while a number enables retry-exhaustion failure for bounded automation. Cancellation aborts both a pending retry and an active session.

On Windows, `prevent_system_sleep: true` creates a system-required execution request for the lifetime of each `serial.tail` job. This prevents idle sleep while monitoring but still lets the display turn off. It cannot override explicit sleep initiated by closing the lid, pressing the power button, or choosing Sleep; after those cases the reconnect path is still required, and logs produced while the machine is actually suspended cannot be recovered without device-side buffering.

This follows the Windows [`SetThreadExecutionState`](https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate) contract and clears the continuous request on the same dedicated thread when the job ends.

## Windows smoke checklist

From the unpacked package, run the guided smoke with the physical location copied from the runtime-smoke report. A deliberately stale fallback such as `COM404` proves that selector-based rediscovery is being used:

```powershell
.\windows-serial-smoke.ps1 `
  -ServerPath .\device-hub-server.exe `
  -SerialPort COM404 `
  -SelectorPhysicalLocation '<physical_location from windows-runtime-smoke.json>'
```

The script creates an isolated temporary config, starts and later stops the packaged server, checks the capability and COM discovery APIs, starts one `serial.tail`, locks Windows for an observation interval, guides the unplug/replug actions, verifies the disconnected/reconnecting/reconnected evidence and endpoint transitions, requires a post-reconnect stdout event, cancels the job, checks `job.json`, `events.ndjson`, and `raw-stream.log`, and writes `windows-serial-smoke.json`. Preserve that report as the release evidence. For an already-running server, omit `-ServerPath` and use `-BaseUrl` plus `-DeviceId`.

For unattended orchestration, add `-AutomaticDeviceTransitions`. The script then detects removal and reconnection from job events instead of waiting for `Read-Host`; the operator still performs the physical unplug/replug when prompted externally.

To cover `serial.write` and `serial.exec`, pass commands that are known to be safe for the connected target. These parameters are intentionally opt-in because Device Hub cannot infer whether an arbitrary serial command changes device state:

```powershell
.\windows-serial-smoke.ps1 `
  -ServerPath .\device-hub-server.exe `
  -SerialPort COM404 `
  -SelectorPhysicalLocation '<physical_location>' `
  -WritePayload "help`r`n" `
  -ExecCommand "version" `
  -ExecUntil '<expected marker>'
```

- Start the serial-only server and open the embedded UI.
- Confirm `/api/capabilities` reports serial available and SSH/J-Link/ADB not compiled.
- Enumerate COM ports and confirm VID, PID, and serial number where the driver exposes them.
- Start `serial.tail`, receive output, unplug/replug the CH340, and confirm the same job resumes.
- With target-safe inputs, confirm `serial.write` succeeds and `serial.exec` observes the requested marker.
- Confirm disconnect and reconnect events appear in the job evidence.
- Cancel while connected and while waiting for the device; both must end as `cancelled`.
- Verify registry and artifacts persist on a Windows path.

Adapter-specific profiles add their own smoke checks for `ssh.exe`, `JLink.exe`, or `adb.exe`; they are not prerequisites for the serial-only release.

## Run at sign-in or as a service

For an operator workstation, Windows Task Scheduler is the simplest deployment: trigger the packaged executable at sign-in, set its working directory to the unpacked package directory, and select “Run whether user is logged on or not” when unattended capture is required.

The executable is currently a console server, not a native Windows Service and does not implement the Service Control Manager protocol. Do not register it directly with `sc.exe create`. For service-style deployment, use a supervisor such as WinSW/NSSM or add a dedicated service-host wrapper in a follow-up. Whichever method is used, grant the service identity access to the configuration, registry/artifact directories, and target COM ports, and validate the same disconnect/reconnect smoke checklist under that identity.
