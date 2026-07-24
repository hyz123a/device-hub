param(
    [Parameter(Mandatory = $true)]
    [string]$ServerPath,
    [int]$Port = 32145,
    [string]$SerialPort = "",
    [int]$Baud = 115200,
    [string]$SelectorPhysicalLocation = "",
    [string]$ReportPath = "windows-runtime-smoke.json"
)

$ErrorActionPreference = "Stop"
$ResolvedServerPath = (Resolve-Path $ServerPath).Path
$ResolvedReportPath = [System.IO.Path]::GetFullPath($ReportPath)
$SmokeDirectory = Join-Path $env:TEMP "device-hub-runtime-smoke-$PID"
$ServerCopy = Join-Path $SmokeDirectory "device-hub-server.exe"
$ConfigPath = Join-Path $SmokeDirectory "devices.yaml"
$RegistryPath = Join-Path $SmokeDirectory "device-registry.yaml"
$ArtifactsDirectory = Join-Path $SmokeDirectory "artifacts"
$Process = $null
$Report = [ordered]@{
    started_at = (Get-Date).ToUniversalTime().ToString("o")
    completed_at = $null
    server = $ResolvedServerPath
    base_url = "http://127.0.0.1:$Port"
    root = $null
    capabilities = $null
    capability_startup_log = "pending"
    serial_ports = $null
    registry_crud = "pending"
    registry_restart = "pending"
    registry_path = $RegistryPath
    waiting_cancel = "pending"
    evidence_job_id = $null
    serial_open = if ([string]::IsNullOrWhiteSpace($SerialPort)) { "skipped" } else { "pending" }
    serial_job_id = $null
    serial_connected_event = $null
    serial_runtime_endpoint = $null
    serial_events = @()
    evidence = "pending"
    evidence_jobs = @()
    artifact_paths = @()
    artifact_files = @()
    system_awake = if ([string]::IsNullOrWhiteSpace($SerialPort)) { "skipped" } else { "pending" }
    selector_physical_location = $SelectorPhysicalLocation
    frontend_status = $null
    result = "running"
    error = $null
}

function Start-SmokeServer {
    $PreviousBindAddress = $env:DEVICE_HUB_BIND_ADDR
    $PreviousConfig = $env:DEVICE_HUB_CONFIG
    $env:DEVICE_HUB_BIND_ADDR = "127.0.0.1:$Port"
    $env:DEVICE_HUB_CONFIG = $ConfigPath
    try {
        $script:Process = Start-Process `
            -FilePath $ServerCopy `
            -WorkingDirectory $SmokeDirectory `
            -RedirectStandardOutput (Join-Path $SmokeDirectory "stdout.log") `
            -RedirectStandardError (Join-Path $SmokeDirectory "stderr.log") `
            -PassThru
    }
    finally {
        $env:DEVICE_HUB_BIND_ADDR = $PreviousBindAddress
        $env:DEVICE_HUB_CONFIG = $PreviousConfig
    }

    $Deadline = (Get-Date).AddSeconds(30)
    $Root = $null
    do {
        if ($script:Process.HasExited) {
            $Stderr = Get-Content (Join-Path $SmokeDirectory "stderr.log") -Raw -ErrorAction SilentlyContinue
            throw "Server exited before becoming ready (exit $($script:Process.ExitCode)): $Stderr"
        }
        try {
            $Root = Invoke-RestMethod -Uri "$($Report.base_url)/api" -Method Get
            break
        }
        catch {
            Start-Sleep -Milliseconds 200
        }
    } while ((Get-Date) -lt $Deadline)
    if ($null -eq $Root) {
        throw "Server did not become ready within 30 seconds"
    }
    $Root
}

function Stop-SmokeServer {
    if ($null -ne $script:Process -and -not $script:Process.HasExited) {
        Stop-Process -Id $script:Process.Id -Force
        $script:Process.WaitForExit()
    }
    $script:Process = $null
}

try {
    New-Item -ItemType Directory -Force -Path $SmokeDirectory | Out-Null
    Copy-Item $ResolvedServerPath $ServerCopy -Force
    $EscapedRegistryPath = $RegistryPath.Replace("'", "''")
    $EscapedArtifactsDirectory = $ArtifactsDirectory.Replace("'", "''")
    $DevicesYaml = "devices: []"
    if (-not [string]::IsNullOrWhiteSpace($SerialPort)) {
        $SelectorYaml = ""
        if (-not [string]::IsNullOrWhiteSpace($SelectorPhysicalLocation)) {
            $EscapedLocation = $SelectorPhysicalLocation.Replace("'", "''")
            $SelectorYaml = @"

      selector:
        physical_location: '$EscapedLocation'
"@
        }
        $DevicesYaml = @"
devices:
  - id: smoke-serial
    label: Windows runtime smoke
    serial:
      port: $SerialPort
      baud: $Baud$SelectorYaml
"@
    }
    $ConfigText = @"
adapters:
  serial: true
  ssh: false
  jlink: false
  adb: false
device_registry_path: '$EscapedRegistryPath'
artifacts_dir: '$EscapedArtifactsDirectory'
$DevicesYaml
"@
    [System.IO.File]::WriteAllText(
        $ConfigPath,
        $ConfigText,
        (New-Object System.Text.UTF8Encoding($false))
    )

    $Root = Start-SmokeServer

    $Capabilities = Invoke-RestMethod -Uri "$($Report.base_url)/api/capabilities" -Method Get
    $Serial = $Capabilities.adapters | Where-Object { $_.adapter_id -eq "serial" }
    if ($null -eq $Serial -or -not $Serial.compiled -or -not $Serial.available) {
        throw "Serial capability is not compiled and available"
    }
    foreach ($AdapterId in @("ssh", "jlink", "adb")) {
        $Adapter = $Capabilities.adapters | Where-Object { $_.adapter_id -eq $AdapterId }
        if ($null -eq $Adapter -or $Adapter.compiled -or $Adapter.available) {
            throw "Serial-only binary unexpectedly exposes $AdapterId"
        }
    }
    $StartupLog = Get-Content (Join-Path $SmokeDirectory "stderr.log") -Raw
    if ($StartupLog -notmatch "device-hub capabilities:.*serial=available" -or
        $StartupLog -notmatch "ssh=not_compiled" -or
        $StartupLog -notmatch "jlink=not_compiled" -or
        $StartupLog -notmatch "adb=not_compiled") {
        throw "Startup log did not report the final capability registration state"
    }
    $Report.capability_startup_log = "passed"

    $Frontend = Invoke-WebRequest -Uri "$($Report.base_url)/" -UseBasicParsing
    if ($Frontend.StatusCode -ne 200 -or $Frontend.Content -notmatch "<html") {
        throw "Embedded frontend did not return HTML"
    }

    $Report.root = $Root
    $Report.capabilities = $Capabilities
    $Report.serial_ports = @(Invoke-RestMethod -Uri "$($Report.base_url)/api/host/serial-ports")
    $Report.frontend_status = $Frontend.StatusCode

    $RegistryDevice = [ordered]@{
        id = "smoke-registry"
        label = "Windows registry create"
        ssh = $null
        serial = [ordered]@{
            port = "COM404"
            baud = 115200
        }
        jlink = $null
        adb = $null
    }
    Invoke-RestMethod `
        -Uri "$($Report.base_url)/api/devices" `
        -Method Post `
        -ContentType "application/json" `
        -Body ($RegistryDevice | ConvertTo-Json -Depth 8) | Out-Null
    $RegistryDevice.label = "Windows registry updated"
    Invoke-RestMethod `
        -Uri "$($Report.base_url)/api/devices/smoke-registry" `
        -Method Put `
        -ContentType "application/json" `
        -Body ($RegistryDevice | ConvertTo-Json -Depth 8) | Out-Null
    if (-not (Test-Path $RegistryPath) -or
        (Get-Content $RegistryPath -Raw) -notmatch "Windows registry updated") {
        throw "Create/update did not persist to the Windows registry path"
    }

    Stop-SmokeServer
    $Root = Start-SmokeServer
    $PersistedDevice = Invoke-RestMethod `
        -Uri "$($Report.base_url)/api/devices/smoke-registry" `
        -Method Get
    if ($PersistedDevice.device.label -ne "Windows registry updated") {
        throw "Updated device did not survive a server restart"
    }
    $Report.registry_restart = "passed"

    $EvidenceStarted = Invoke-RestMethod `
        -Uri "$($Report.base_url)/api/devices/smoke-registry/actions/serial.tail" `
        -Method Post `
        -ContentType "application/json" `
        -Body "null"
    $Report.evidence_job_id = $EvidenceStarted.id
    $Deadline = (Get-Date).AddSeconds(15)
    $WaitingObserved = $false
    do {
        $EvidenceJob = Invoke-RestMethod `
            -Uri "$($Report.base_url)/api/jobs/$($EvidenceStarted.id)" `
            -Method Get
        $EvidenceEvents = @(
            Invoke-RestMethod `
                -Uri "$($Report.base_url)/api/jobs/$($EvidenceStarted.id)/events" `
                -Method Get
        )
        $WaitingObserved = $null -ne (
            $EvidenceEvents |
                Where-Object { $_.message -match "^waiting_for_device:" } |
                Select-Object -Last 1
        )
        if ($WaitingObserved) {
            break
        }
        if ($EvidenceJob.status -ne "running") {
            throw "Missing-port evidence job unexpectedly became $($EvidenceJob.status)"
        }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $Deadline)
    if (-not $WaitingObserved) {
        throw "Missing COM port did not enter waiting_for_device"
    }
    $EvidenceCancelled = Invoke-RestMethod `
        -Uri "$($Report.base_url)/api/jobs/$($EvidenceStarted.id)" `
        -Method Delete
    if ($EvidenceCancelled.status -ne "cancelled") {
        throw "Waiting serial tail did not cancel cleanly: $($EvidenceCancelled.status)"
    }
    $Report.waiting_cancel = "passed"

    $PersistedEvidence = Invoke-RestMethod `
        -Uri "$($Report.base_url)/api/jobs/$($EvidenceStarted.id)/evidence" `
        -Method Get
    $Report.evidence_jobs += $PersistedEvidence
    foreach ($ArtifactName in @("events.ndjson", "job.json")) {
        $Artifact = $PersistedEvidence.artifacts |
            Where-Object { $_.name -eq $ArtifactName } |
            Select-Object -Last 1
        if ($null -eq $Artifact) {
            throw "Job evidence did not list $ArtifactName"
        }
        $ArtifactPath = Join-Path (Join-Path $ArtifactsDirectory "jobs") `
            (Join-Path $EvidenceStarted.id $ArtifactName)
        if (-not (Test-Path $ArtifactPath)) {
            throw "Expected Windows artifact path does not exist: $ArtifactPath"
        }
        $Report.artifact_paths += $ArtifactPath
        $ArtifactFile = Get-Item $ArtifactPath
        $Report.artifact_files += [ordered]@{
            job_id = $EvidenceStarted.id
            name = $ArtifactName
            path = $ArtifactPath
            length = $ArtifactFile.Length
            sha256 = $null
        }
    }
    $DownloadedJob = Invoke-WebRequest `
        -Uri "$($Report.base_url)/api/jobs/$($EvidenceStarted.id)/artifacts/job.json" `
        -UseBasicParsing
    if ($DownloadedJob.StatusCode -ne 200 -or
        $DownloadedJob.Content -notmatch $EvidenceStarted.id) {
        throw "Persisted job artifact was not downloadable"
    }
    $Report.evidence = "passed"

    Invoke-RestMethod `
        -Uri "$($Report.base_url)/api/devices/smoke-registry" `
        -Method Delete | Out-Null
    $DevicesAfterDelete = @(Invoke-RestMethod -Uri "$($Report.base_url)/api/devices")
    if ($DevicesAfterDelete | Where-Object { $_.id -eq "smoke-registry" }) {
        throw "Deleted device is still returned by the registry"
    }
    if ((Get-Content $RegistryPath -Raw) -match "smoke-registry") {
        throw "Delete did not persist to the Windows registry path"
    }
    $Report.registry_crud = "passed"

    if (-not [string]::IsNullOrWhiteSpace($SerialPort)) {
        $Started = Invoke-RestMethod `
            -Uri "$($Report.base_url)/api/devices/smoke-serial/actions/serial.tail" `
            -Method Post `
            -ContentType "application/json" `
            -Body "null"
        $Report.serial_job_id = $Started.id
        $Deadline = (Get-Date).AddSeconds(15)
        $Connected = $false
        do {
            $CurrentJob = Invoke-RestMethod -Uri "$($Report.base_url)/api/jobs/$($Started.id)"
            $Events = Invoke-RestMethod -Uri "$($Report.base_url)/api/jobs/$($Started.id)/events"
            $ConnectedEvent = $Events |
                Where-Object { $_.message -match "^serial connected:" } |
                Select-Object -Last 1
            $Connected = $null -ne $ConnectedEvent
            if ($Connected) {
                break
            }
            if ($CurrentJob.status -eq "failed") {
                throw "Failed to open $SerialPort"
            }
            Start-Sleep -Milliseconds 200
        } while ((Get-Date) -lt $Deadline)
        if (-not $Connected) {
            throw "Timed out opening $SerialPort"
        }
        $Report.serial_connected_event = $ConnectedEvent.message
        $Report.serial_events = @($Events)
        $DeviceDetail = Invoke-RestMethod -Uri "$($Report.base_url)/api/devices/smoke-serial"
        $ActiveTail = $DeviceDetail.runtime.active_tails |
            Where-Object { $_.job_id -eq $Started.id } |
            Select-Object -Last 1
        if ($null -eq $ActiveTail -or [string]::IsNullOrWhiteSpace($ActiveTail.current_endpoint)) {
            throw "Device runtime did not expose the resolved serial endpoint"
        }
        $Report.serial_runtime_endpoint = $ActiveTail.current_endpoint
        $AwakeEvent = $Events |
            Where-Object {
                $_.message -eq "Windows system sleep inhibition active for serial.tail"
            } |
            Select-Object -Last 1
        if ($null -eq $AwakeEvent) {
            throw "Windows system sleep inhibition was not activated"
        }
        $Report.system_awake = "passed"

        $Cancelled = Invoke-RestMethod `
            -Uri "$($Report.base_url)/api/jobs/$($Started.id)" `
            -Method Delete
        if ($Cancelled.status -ne "cancelled") {
            throw "Serial open smoke did not cancel cleanly: $($Cancelled.status)"
        }

        $Evidence = Invoke-RestMethod `
            -Uri "$($Report.base_url)/api/jobs/$($Started.id)/evidence" `
            -Method Get
        $Report.evidence_jobs += $Evidence
        $RequiredArtifacts = @("events.ndjson", "job.json")
        foreach ($ArtifactName in $RequiredArtifacts) {
            $Artifact = $Evidence.artifacts |
                Where-Object { $_.name -eq $ArtifactName } |
                Select-Object -Last 1
            if ($null -eq $Artifact) {
                throw "Job evidence did not list $ArtifactName"
            }
            $ArtifactPath = Join-Path (Join-Path $ArtifactsDirectory "jobs") `
                (Join-Path $Started.id $ArtifactName)
            if (-not (Test-Path $ArtifactPath)) {
                throw "Expected Windows artifact path does not exist: $ArtifactPath"
            }
            $Report.artifact_paths += $ArtifactPath
            $ArtifactFile = Get-Item $ArtifactPath
            $Report.artifact_files += [ordered]@{
                job_id = $Started.id
                name = $ArtifactName
                path = $ArtifactPath
                length = $ArtifactFile.Length
                sha256 = $null
            }
        }
        $DownloadedJob = Invoke-WebRequest `
            -Uri "$($Report.base_url)/api/jobs/$($Started.id)/artifacts/job.json" `
            -UseBasicParsing
        if ($DownloadedJob.StatusCode -ne 200 -or $DownloadedJob.Content -notmatch $Started.id) {
            throw "Persisted job artifact was not downloadable"
        }
        $Report.serial_open = "passed"
    }

    $Report.result = "passed"
}
catch {
    $Report.result = "failed"
    $Report.error = $_.Exception.Message
    throw
}
finally {
    Stop-SmokeServer
    foreach ($ArtifactFile in $Report.artifact_files) {
        if (Test-Path $ArtifactFile.path) {
            $ArtifactFile.sha256 = (
                Get-FileHash $ArtifactFile.path -Algorithm SHA256
            ).Hash.ToLowerInvariant()
        }
    }
    $Report.completed_at = (Get-Date).ToUniversalTime().ToString("o")
    $Report | ConvertTo-Json -Depth 12 | Set-Content -Path $ResolvedReportPath -Encoding UTF8
    Remove-Item $SmokeDirectory -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "Runtime smoke report: $ResolvedReportPath"
}
