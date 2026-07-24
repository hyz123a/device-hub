param(
    [string]$BaseUrl = "http://127.0.0.1:3000",
    [string]$DeviceId = "smoke-serial",
    [string]$ServerPath = "",
    [string]$SerialPort = "",
    [int]$Baud = 115200,
    [string]$SelectorPhysicalLocation = "",
    [int]$ManagedPort = 32150,
    [int]$EventTimeoutSeconds = 120,
    [int]$LockObservationSeconds = 30,
    [switch]$SkipLock,
    [switch]$AutomaticDeviceTransitions,
    [string]$WritePayload = "",
    [string]$ExecCommand = "",
    [string]$ExecUntil = "",
    [int]$ExecTimeoutSeconds = 10,
    [string]$ReportPath = "windows-serial-smoke.json"
)

$ErrorActionPreference = "Stop"
if (-not [string]::IsNullOrWhiteSpace($ServerPath)) {
    $BaseUrl = "http://127.0.0.1:$ManagedPort"
}
$BaseUrl = $BaseUrl.TrimEnd("/")
$JobId = $null
$ManagedProcess = $null
$ManagedDirectory = $null
$StartedAt = (Get-Date).ToUniversalTime()
$Report = [ordered]@{
    started_at = $StartedAt.ToString("o")
    completed_at = $null
    base_url = $BaseUrl
    device_id = $DeviceId
    server = $ServerPath
    serial_port = $SerialPort
    selector_physical_location = $SelectorPhysicalLocation
    host = [ordered]@{
        computer_name = $env:COMPUTERNAME
        os = [System.Environment]::OSVersion.VersionString
        powershell = $PSVersionTable.PSVersion.ToString()
    }
    capabilities = $null
    serial_ports_before = $null
    serial_ports_after = $null
    job_id = $null
    serial_write = if ([string]::IsNullOrEmpty($WritePayload)) { "skipped" } else { "pending" }
    serial_write_job_id = $null
    serial_exec = if ([string]::IsNullOrWhiteSpace($ExecCommand)) { "skipped" } else { "pending" }
    serial_exec_result = $null
    endpoint_before_disconnect = $null
    endpoint_while_disconnected = $null
    endpoint_after_reconnect = $null
    lock_check = if ($SkipLock) { "skipped" } else { "pending" }
    disconnect_observed = $false
    reconnect_observed = $false
    cancelled = $false
    events = @()
    evidence = $null
    raw_stream_artifact = "pending"
    result = "running"
    error = $null
}

function Invoke-DeviceHub {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [ValidateSet("Get", "Post", "Delete")]
        [string]$Method = "Get",
        [string]$Body
    )

    $Parameters = @{
        Uri = "$BaseUrl$Path"
        Method = $Method
    }
    if ($PSBoundParameters.ContainsKey("Body")) {
        $Parameters.ContentType = "application/json"
        $Parameters.Body = $Body
    }
    Invoke-RestMethod @Parameters
}

function Get-JobEvents {
    param([string]$Id)
    @(Invoke-DeviceHub -Path "/api/jobs/$Id/events")
}

function Wait-JobEvent {
    param(
        [string]$Id,
        [string]$Pattern,
        [int]$TimeoutSeconds
    )

    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $Events = Get-JobEvents -Id $Id
        $Match = $Events | Where-Object { $_.message -match $Pattern } | Select-Object -Last 1
        if ($null -ne $Match) {
            return $Match
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $Deadline)

    throw "Timed out waiting for job event matching: $Pattern"
}

function Wait-JobTerminal {
    param(
        [string]$Id,
        [int]$TimeoutSeconds
    )

    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $Job = Invoke-DeviceHub -Path "/api/jobs/$Id"
        if ($Job.status -in @("succeeded", "failed", "cancelled")) {
            return $Job
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $Deadline)

    throw "Timed out waiting for job $Id to finish"
}

function Wait-JobChannel {
    param(
        [string]$Id,
        [string]$Channel,
        [string]$After = "",
        [int]$TimeoutSeconds
    )

    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $Events = Get-JobEvents -Id $Id
        $Match = $Events |
            Where-Object {
                $_.channel -eq $Channel -and
                ([string]::IsNullOrWhiteSpace($After) -or
                    [DateTimeOffset]$_.ts -gt [DateTimeOffset]$After)
            } |
            Select-Object -Last 1
        if ($null -ne $Match) {
            return $Match
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $Deadline)

    throw "Timed out waiting for a $Channel event from job $Id"
}

try {
    if (-not [string]::IsNullOrWhiteSpace($ServerPath)) {
        if ([string]::IsNullOrWhiteSpace($SerialPort)) {
            throw "-SerialPort is required when -ServerPath is used"
        }

        $ResolvedServerPath = (Resolve-Path $ServerPath).Path
        $ManagedDirectory = Join-Path $env:TEMP "device-hub-serial-smoke-$PID"
        $ServerCopy = Join-Path $ManagedDirectory "device-hub-server.exe"
        $ConfigPath = Join-Path $ManagedDirectory "devices.yaml"
        New-Item -ItemType Directory -Force -Path $ManagedDirectory | Out-Null
        Copy-Item $ResolvedServerPath $ServerCopy -Force

        $SelectorYaml = ""
        if (-not [string]::IsNullOrWhiteSpace($SelectorPhysicalLocation)) {
            $EscapedLocation = $SelectorPhysicalLocation.Replace("'", "''")
            $SelectorYaml = @"

      selector:
        physical_location: '$EscapedLocation'
"@
        }
        $ConfigText = @"
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
  - id: $DeviceId
    label: Windows guided serial smoke
    serial:
      port: $SerialPort
      baud: $Baud$SelectorYaml
"@
        [System.IO.File]::WriteAllText(
            $ConfigPath,
            $ConfigText,
            (New-Object System.Text.UTF8Encoding($false))
        )

        $PreviousBindAddress = $env:DEVICE_HUB_BIND_ADDR
        $PreviousConfig = $env:DEVICE_HUB_CONFIG
        $env:DEVICE_HUB_BIND_ADDR = "127.0.0.1:$ManagedPort"
        $env:DEVICE_HUB_CONFIG = $ConfigPath
        try {
            $ManagedProcess = Start-Process `
                -FilePath $ServerCopy `
                -WorkingDirectory $ManagedDirectory `
                -RedirectStandardOutput (Join-Path $ManagedDirectory "stdout.log") `
                -RedirectStandardError (Join-Path $ManagedDirectory "stderr.log") `
                -PassThru
        }
        finally {
            $env:DEVICE_HUB_BIND_ADDR = $PreviousBindAddress
            $env:DEVICE_HUB_CONFIG = $PreviousConfig
        }

        $ReadyDeadline = (Get-Date).AddSeconds(30)
        do {
            if ($ManagedProcess.HasExited) {
                $Stderr = Get-Content (Join-Path $ManagedDirectory "stderr.log") -Raw -ErrorAction SilentlyContinue
                throw "Server exited before becoming ready (exit $($ManagedProcess.ExitCode)): $Stderr"
            }
            try {
                Invoke-DeviceHub -Path "/api" | Out-Null
                $Ready = $true
                break
            }
            catch {
                Start-Sleep -Milliseconds 200
            }
        } while ((Get-Date) -lt $ReadyDeadline)
        if (-not $Ready) {
            throw "Server did not become ready within 30 seconds"
        }
    }

    $Capabilities = Invoke-DeviceHub -Path "/api/capabilities"
    $Report.capabilities = $Capabilities
    $SerialCapability = $Capabilities.adapters | Where-Object { $_.adapter_id -eq "serial" }
    if ($null -eq $SerialCapability -or -not $SerialCapability.available) {
        throw "Serial capability is not available"
    }

    $Report.serial_ports_before = @(Invoke-DeviceHub -Path "/api/host/serial-ports")
    $Started = Invoke-DeviceHub `
        -Path "/api/devices/$DeviceId/actions/serial.tail" `
        -Method Post `
        -Body "null"
    $JobId = $Started.id
    $Report.job_id = $JobId
    if ([string]::IsNullOrWhiteSpace($JobId)) {
        throw "serial.tail response did not contain a job id"
    }

    Wait-JobEvent -Id $JobId -Pattern "^serial connected:" -TimeoutSeconds $EventTimeoutSeconds | Out-Null
    $ConnectedDetail = Invoke-DeviceHub -Path "/api/devices/$DeviceId"
    $ConnectedTail = $ConnectedDetail.runtime.active_tails |
        Where-Object { $_.job_id -eq $JobId } |
        Select-Object -Last 1
    if ($null -eq $ConnectedTail -or [string]::IsNullOrWhiteSpace($ConnectedTail.current_endpoint)) {
        throw "Connected tail did not expose its current endpoint"
    }
    $Report.endpoint_before_disconnect = $ConnectedTail.current_endpoint

    if (-not [string]::IsNullOrEmpty($WritePayload)) {
        $WriteBody = @{
            payload = $WritePayload
        } | ConvertTo-Json
        $WriteJob = Invoke-DeviceHub `
            -Path "/api/devices/$DeviceId/actions/serial.write" `
            -Method Post `
            -Body $WriteBody
        $Report.serial_write_job_id = $WriteJob.id
        $FinishedWrite = Wait-JobTerminal `
            -Id $WriteJob.id `
            -TimeoutSeconds $EventTimeoutSeconds
        if ($FinishedWrite.status -ne "succeeded") {
            throw "serial.write did not succeed: $($FinishedWrite.status)"
        }
        $Report.serial_write = "passed"
    }

    if (-not [string]::IsNullOrWhiteSpace($ExecCommand)) {
        $ExecRequest = [ordered]@{
            command = $ExecCommand
            timeout_secs = $ExecTimeoutSeconds
        }
        if (-not [string]::IsNullOrEmpty($ExecUntil)) {
            $ExecRequest.until = $ExecUntil
        }
        $ExecResult = Invoke-DeviceHub `
            -Path "/api/devices/$DeviceId/serial/exec" `
            -Method Post `
            -Body ($ExecRequest | ConvertTo-Json)
        if (-not [string]::IsNullOrEmpty($ExecUntil) -and
            ($ExecResult.timed_out -or $ExecResult.matched -ne $ExecUntil)) {
            throw "serial.exec did not observe the requested marker: $ExecUntil"
        }
        $Report.serial_exec_result = $ExecResult
        $Report.serial_exec = "passed"
    }

    if (-not $SkipLock) {
        Write-Host "Windows will lock now. Unlock after at least $LockObservationSeconds seconds."
        Start-Process -FilePath "rundll32.exe" -ArgumentList "user32.dll,LockWorkStation"
        Start-Sleep -Seconds $LockObservationSeconds
        $CurrentJob = Invoke-DeviceHub -Path "/api/jobs/$JobId"
        if ($CurrentJob.status -ne "running") {
            throw "serial.tail stopped during the lock observation: $($CurrentJob.status)"
        }
        $Report.lock_check = "passed"
    }

    if ($AutomaticDeviceTransitions) {
        Write-Host "Waiting for the target USB serial adapter to be unplugged."
    }
    else {
        Read-Host "Unplug the target USB serial adapter, wait until Windows removes the COM port, then press Enter"
    }
    Wait-JobEvent -Id $JobId -Pattern "^serial disconnected" -TimeoutSeconds $EventTimeoutSeconds | Out-Null
    $DisconnectedDetail = Invoke-DeviceHub -Path "/api/devices/$DeviceId"
    $DisconnectedTail = $DisconnectedDetail.runtime.active_tails |
        Where-Object { $_.job_id -eq $JobId } |
        Select-Object -Last 1
    if ($null -eq $DisconnectedTail) {
        throw "serial.tail disappeared while waiting for the device"
    }
    $Report.endpoint_while_disconnected = $DisconnectedTail.current_endpoint
    if (-not [string]::IsNullOrWhiteSpace($DisconnectedTail.current_endpoint)) {
        throw "Disconnected tail still reports a stale current endpoint: $($DisconnectedTail.current_endpoint)"
    }
    $Report.disconnect_observed = $true

    if ($AutomaticDeviceTransitions) {
        Write-Host "Waiting for the same adapter to reconnect and emit at least one log line."
    }
    else {
        Read-Host "Reconnect the same adapter and generate at least one log line, then press Enter"
    }
    $ReconnectEvent = Wait-JobEvent `
        -Id $JobId `
        -Pattern "^serial reconnected:.*outage_ms=.*total_disconnected_ms=" `
        -TimeoutSeconds $EventTimeoutSeconds
    Wait-JobChannel `
        -Id $JobId `
        -Channel "stdout" `
        -After $ReconnectEvent.ts `
        -TimeoutSeconds $EventTimeoutSeconds | Out-Null
    $Report.reconnect_observed = $true
    $Report.serial_ports_after = @(Invoke-DeviceHub -Path "/api/host/serial-ports")
    $ReconnectedDetail = Invoke-DeviceHub -Path "/api/devices/$DeviceId"
    $ReconnectedTail = $ReconnectedDetail.runtime.active_tails |
        Where-Object { $_.job_id -eq $JobId } |
        Select-Object -Last 1
    if ($null -eq $ReconnectedTail -or
        [string]::IsNullOrWhiteSpace($ReconnectedTail.current_endpoint)) {
        throw "Reconnected tail did not republish its current endpoint"
    }
    $Report.endpoint_after_reconnect = $ReconnectedTail.current_endpoint

    $Cancelled = Invoke-DeviceHub -Path "/api/jobs/$JobId" -Method Delete
    $Report.cancelled = $Cancelled.status -eq "cancelled"
    if (-not $Report.cancelled) {
        throw "job cancellation returned status: $($Cancelled.status)"
    }

    $Report.events = @(Get-JobEvents -Id $JobId)
    $Evidence = Invoke-DeviceHub -Path "/api/jobs/$JobId/evidence"
    foreach ($ArtifactName in @("job.json", "events.ndjson", "raw-stream.log")) {
        if (-not ($Evidence.artifacts | Where-Object { $_.name -eq $ArtifactName })) {
            throw "Job evidence did not list required artifact: $ArtifactName"
        }
    }
    $Report.evidence = $Evidence
    $Report.raw_stream_artifact = "passed"
    $Report.result = "passed"
}
catch {
    $Report.result = "failed"
    $Report.error = $_.Exception.Message
    throw
}
finally {
    if ($null -ne $JobId -and -not $Report.cancelled) {
        try {
            $CurrentJob = Invoke-DeviceHub -Path "/api/jobs/$JobId"
            if ($CurrentJob.status -eq "running") {
                $Cancelled = Invoke-DeviceHub -Path "/api/jobs/$JobId" -Method Delete
                $Report.cancelled = $Cancelled.status -eq "cancelled"
            }
            $Report.events = @(Get-JobEvents -Id $JobId)
        }
        catch {
            if ($null -eq $Report.error) {
                $Report.error = "Cleanup failed: $($_.Exception.Message)"
            }
        }
    }

    if ($null -ne $ManagedProcess -and -not $ManagedProcess.HasExited) {
        Stop-Process -Id $ManagedProcess.Id -Force
        $ManagedProcess.WaitForExit()
    }

    $Report.completed_at = (Get-Date).ToUniversalTime().ToString("o")
    $ResolvedReportPath = [System.IO.Path]::GetFullPath($ReportPath)
    $Report | ConvertTo-Json -Depth 12 | Set-Content -Path $ResolvedReportPath -Encoding UTF8
    if ($null -ne $ManagedDirectory) {
        Remove-Item $ManagedDirectory -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-Host "Smoke report: $ResolvedReportPath"
}
