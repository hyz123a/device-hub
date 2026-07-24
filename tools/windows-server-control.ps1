param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("Start", "Foreground", "Stop", "Status", "Logs")]
    [string]$Action,
    [string]$ServerPath = "",
    [string]$ConfigPath = "",
    [string]$RuntimeDirectory = "",
    [string]$BindAddress = "127.0.0.1:3000",
    [int]$StartupTimeoutSeconds = 30,
    [int]$Tail = 100,
    [switch]$Follow
)

$ErrorActionPreference = "Stop"
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[Console]::InputEncoding = $Utf8NoBom
[Console]::OutputEncoding = $Utf8NoBom
$OutputEncoding = $Utf8NoBom

if ([string]::IsNullOrWhiteSpace($RuntimeDirectory)) {
    $RuntimeDirectory = Join-Path $env:LOCALAPPDATA "DeviceHub"
}
$RuntimeDirectory = [System.IO.Path]::GetFullPath($RuntimeDirectory)
$StatePath = Join-Path $RuntimeDirectory "device-hub-process.json"
$PidPath = Join-Path $RuntimeDirectory "device-hub.pid"
$StdoutPath = Join-Path $RuntimeDirectory "stdout.log"
$StderrPath = Join-Path $RuntimeDirectory "stderr.log"
$ManagedServerPath = Join-Path $RuntimeDirectory "device-hub-server.exe"
$ManagedConfigPath = Join-Path $RuntimeDirectory "devices.yaml"

function Read-ProcessState {
    if (-not (Test-Path $StatePath)) {
        return $null
    }
    try {
        Get-Content $StatePath -Raw | ConvertFrom-Json
    }
    catch {
        return $null
    }
}

function Get-ManagedProcess {
    $State = Read-ProcessState
    if ($null -eq $State -or $null -eq $State.pid) {
        return $null
    }

    $Process = Get-Process -Id ([int]$State.pid) -ErrorAction SilentlyContinue
    if ($null -eq $Process) {
        return $null
    }

    try {
        $ActualPath = [System.IO.Path]::GetFullPath($Process.Path)
        $ExpectedPath = [System.IO.Path]::GetFullPath([string]$State.server_path)
        if (-not $ActualPath.Equals($ExpectedPath, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $null
        }
    }
    catch {
        return $null
    }
    $Process
}

function Remove-StaleState {
    Remove-Item $StatePath -Force -ErrorAction SilentlyContinue
    Remove-Item $PidPath -Force -ErrorAction SilentlyContinue
}

function Write-ProcessState {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.Process]$Process,
        [Parameter(Mandatory = $true)]
        [string]$Mode
    )

    $State = [ordered]@{
        pid = $Process.Id
        mode = $Mode
        started_at = (Get-Date).ToUniversalTime().ToString("o")
        server_path = $ManagedServerPath
        config_path = $ManagedConfigPath
        runtime_directory = $RuntimeDirectory
        bind_address = $BindAddress
        stdout_log = $StdoutPath
        stderr_log = $StderrPath
    }
    [System.IO.File]::WriteAllText(
        $StatePath,
        ($State | ConvertTo-Json),
        (New-Object System.Text.UTF8Encoding($false))
    )
    [System.IO.File]::WriteAllText(
        $PidPath,
        "$($Process.Id)`n",
        (New-Object System.Text.UTF8Encoding($false))
    )
}

function Prepare-Runtime {
    if ([string]::IsNullOrWhiteSpace($ServerPath)) {
        throw "ServerPath is required for $Action"
    }
    if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
        throw "ConfigPath is required for $Action"
    }

    $ResolvedServerPath = (Resolve-Path $ServerPath).Path
    $ResolvedConfigPath = (Resolve-Path $ConfigPath).Path
    New-Item -ItemType Directory -Force -Path $RuntimeDirectory | Out-Null

    if (-not $ResolvedServerPath.Equals(
        $ManagedServerPath,
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
        Copy-Item $ResolvedServerPath $ManagedServerPath -Force
    }
    if (-not $ResolvedConfigPath.Equals(
        $ManagedConfigPath,
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
        Copy-Item $ResolvedConfigPath $ManagedConfigPath -Force
    }

    foreach ($LogPath in @($StdoutPath, $StderrPath)) {
        if (Test-Path $LogPath) {
            $Timestamp = Get-Date -Format "yyyyMMdd-HHmmss-fff"
            Move-Item $LogPath "$LogPath.$Timestamp" -Force
        }
    }
}

function Start-ManagedServer {
    param(
        [Parameter(Mandatory = $true)]
        [bool]$InForeground
    )

    $ExistingProcess = Get-ManagedProcess
    if ($null -ne $ExistingProcess) {
        $ExistingState = Read-ProcessState
        Write-Output "Device Hub is already running"
        Write-Output "PID: $($ExistingProcess.Id)"
        Write-Output "URL: http://$($ExistingState.bind_address)"
        Write-Output "Runtime: $RuntimeDirectory"
        return
    }
    Remove-StaleState
    Prepare-Runtime

    $PreviousBindAddress = $env:DEVICE_HUB_BIND_ADDR
    $PreviousConfig = $env:DEVICE_HUB_CONFIG
    $env:DEVICE_HUB_BIND_ADDR = $BindAddress
    $env:DEVICE_HUB_CONFIG = $ManagedConfigPath
    try {
        $StartArguments = @{
            FilePath = $ManagedServerPath
            WorkingDirectory = $RuntimeDirectory
            PassThru = $true
        }
        if ($InForeground) {
            $StartArguments.NoNewWindow = $true
        }
        else {
            $StartArguments.RedirectStandardOutput = $StdoutPath
            $StartArguments.RedirectStandardError = $StderrPath
            $StartArguments.WindowStyle = "Hidden"
        }
        $Process = Start-Process @StartArguments
    }
    finally {
        $env:DEVICE_HUB_BIND_ADDR = $PreviousBindAddress
        $env:DEVICE_HUB_CONFIG = $PreviousConfig
    }

    Write-ProcessState -Process $Process -Mode $(if ($InForeground) { "foreground" } else { "background" })

    $BaseUrl = "http://$BindAddress"
    try {
        $ListenPort = ([Uri]"tcp://$BindAddress").Port
    }
    catch {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        Remove-StaleState
        throw "Invalid bind address: $BindAddress"
    }
    $Deadline = (Get-Date).AddSeconds($StartupTimeoutSeconds)
    $Ready = $false
    do {
        $Process.Refresh()
        if ($Process.HasExited) {
            $Process.WaitForExit()
            $ErrorText = if (Test-Path $StderrPath) {
                Get-Content $StderrPath -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
            }
            else {
                ""
            }
            Remove-StaleState
            throw "Device Hub exited before becoming ready: $ErrorText"
        }
        try {
            $OwnedListener = Get-NetTCPConnection `
                -LocalPort $ListenPort `
                -State Listen `
                -ErrorAction SilentlyContinue |
                Where-Object { $_.OwningProcess -eq $Process.Id } |
                Select-Object -First 1
            if ($null -ne $OwnedListener) {
                Invoke-WebRequest -Uri "$BaseUrl/api" -UseBasicParsing -TimeoutSec 2 | Out-Null
                $Process.Refresh()
                $Ready = -not $Process.HasExited
            }
        }
        catch {
        }
        if (-not $Ready) {
            Start-Sleep -Milliseconds 200
        }
    } while (-not $Ready -and (Get-Date) -lt $Deadline)

    if (-not $Ready) {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        Remove-StaleState
        throw "Device Hub did not become ready at $BaseUrl within $StartupTimeoutSeconds seconds"
    }

    Write-Output "Device Hub started"
    Write-Output "PID: $($Process.Id)"
    Write-Output "URL: $BaseUrl"
    Write-Output "Runtime: $RuntimeDirectory"
    if (-not $InForeground) {
        Write-Output "stdout: $StdoutPath"
        Write-Output "stderr: $StderrPath"
        return
    }

    Write-Output "Running in foreground; press Ctrl+C to stop."
    try {
        $Process.WaitForExit()
        if ((Test-Path $StatePath) -and $Process.ExitCode -ne 0) {
            throw "Device Hub exited with code $($Process.ExitCode)"
        }
    }
    finally {
        if (-not $Process.HasExited) {
            Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        }
        Remove-StaleState
    }
}

try {
    switch ($Action) {
        "Start" {
            Start-ManagedServer -InForeground $false
        }
        "Foreground" {
            Start-ManagedServer -InForeground $true
        }
        "Stop" {
            $Process = Get-ManagedProcess
            if ($null -eq $Process) {
                Remove-StaleState
                Write-Output "Device Hub is not running"
                exit 0
            }
            Stop-Process -Id $Process.Id -Force
            Remove-StaleState
            $Process.WaitForExit()
            Write-Output "Device Hub stopped (PID $($Process.Id))"
        }
        "Status" {
            $State = Read-ProcessState
            $Process = Get-ManagedProcess
            if ($null -eq $Process) {
                Remove-StaleState
                Write-Output "Device Hub is stopped"
                exit 3
            }
            Write-Output "Device Hub is running"
            Write-Output "PID: $($Process.Id)"
            Write-Output "URL: http://$($State.bind_address)"
            Write-Output "Runtime: $RuntimeDirectory"
            Write-Output "Started: $($State.started_at)"
        }
        "Logs" {
            if (-not (Test-Path $StdoutPath) -and -not (Test-Path $StderrPath)) {
                throw "No Device Hub logs found in $RuntimeDirectory"
            }
            $Paths = @($StdoutPath, $StderrPath) | Where-Object { Test-Path $_ }
            Get-Content -Path $Paths -Tail $Tail -Wait:$Follow -Encoding UTF8
        }
    }
}
catch {
    [Console]::Error.WriteLine("error: $($_.Exception.Message)")
    exit 1
}
