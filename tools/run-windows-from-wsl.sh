#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -f "$SCRIPT_DIR/windows-server-control.ps1" ] &&
  [ -f "$SCRIPT_DIR/device-hub-server.exe" ]; then
  ROOT_DIR=$SCRIPT_DIR
  CONTROL_SCRIPT="$SCRIPT_DIR/windows-server-control.ps1"
  DEFAULT_SERVER="$SCRIPT_DIR/device-hub-server.exe"
else
  ROOT_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
  CONTROL_SCRIPT="$ROOT_DIR/tools/windows-server-control.ps1"
  DEFAULT_SERVER="$ROOT_DIR/target/x86_64-pc-windows-msvc/release/device-hub-server.exe"
  if [ ! -f "$DEFAULT_SERVER" ]; then
    DEFAULT_SERVER="$ROOT_DIR/target/packages/device-hub-windows-serial/device-hub-server.exe"
  fi
fi
DEFAULT_CONFIG="$ROOT_DIR/devices.windows.yaml"
if [ ! -f "$DEFAULT_CONFIG" ]; then
  DEFAULT_CONFIG="$ROOT_DIR/devices.example.yaml"
fi

usage() {
  cat <<'EOF'
Usage:
  tools/run-windows-from-wsl.sh start [options]
  tools/run-windows-from-wsl.sh foreground [options]
  tools/run-windows-from-wsl.sh stop [--runtime-dir PATH]
  tools/run-windows-from-wsl.sh status [--runtime-dir PATH]
  tools/run-windows-from-wsl.sh restart [options]
  tools/run-windows-from-wsl.sh logs [--runtime-dir PATH] [--tail N] [--follow]

Options:
  --server PATH         Windows EXE path; defaults to the cross-built release
  --config PATH         YAML config; defaults to devices.windows.yaml when present,
                        otherwise devices.example.yaml
  --runtime-dir PATH    Windows-local runtime directory; defaults to
                        %LOCALAPPDATA%\DeviceHub
  --bind ADDRESS        Listen address (default: 127.0.0.1:3001)
  --timeout SECONDS     Startup readiness timeout (default: 30)
  --tail LINES          Lines shown by the logs command (default: 100)
  --follow              Follow logs until Ctrl+C
  -h, --help            Show this help

WSL paths (/home/... or /mnt/c/...) and Windows paths (C:\...) are accepted.
The server and config are copied into the Windows-local runtime directory.
EOF
}

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

to_windows_path() {
  case "$1" in
    [A-Za-z]:\\*|\\\\*)
      printf '%s\n' "$1"
      ;;
    *)
      wslpath -aw "$1"
      ;;
  esac
}

command_name=${1:-}
case "$command_name" in
  start|foreground|stop|status|restart|logs)
    shift
    ;;
  -h|--help|"")
    usage
    exit 0
    ;;
  *)
    fail "unknown command: $command_name"
    ;;
esac

server_path=$DEFAULT_SERVER
config_path=$DEFAULT_CONFIG
runtime_dir=
bind_address=127.0.0.1:3001
startup_timeout=30
tail_lines=100
follow=false

while [ "$#" -gt 0 ]; do
  case "$1" in
    --server)
      [ "$#" -ge 2 ] || fail "--server requires a path"
      server_path=$2
      shift 2
      ;;
    --config)
      [ "$#" -ge 2 ] || fail "--config requires a path"
      config_path=$2
      shift 2
      ;;
    --runtime-dir)
      [ "$#" -ge 2 ] || fail "--runtime-dir requires a path"
      runtime_dir=$2
      shift 2
      ;;
    --bind)
      [ "$#" -ge 2 ] || fail "--bind requires an address"
      bind_address=$2
      shift 2
      ;;
    --timeout)
      [ "$#" -ge 2 ] || fail "--timeout requires a number"
      startup_timeout=$2
      shift 2
      ;;
    --tail)
      [ "$#" -ge 2 ] || fail "--tail requires a number"
      tail_lines=$2
      shift 2
      ;;
    --follow)
      follow=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown option: $1"
      ;;
  esac
done

command -v powershell.exe >/dev/null 2>&1 ||
  fail "powershell.exe was not found; this script must run inside WSL"
command -v wslpath >/dev/null 2>&1 ||
  fail "wslpath was not found; this script must run inside WSL"
[ -f "$CONTROL_SCRIPT" ] || fail "PowerShell controller not found: $CONTROL_SCRIPT"

case "$startup_timeout" in *[!0-9]*|"") fail "--timeout must be a positive integer" ;; esac
case "$tail_lines" in *[!0-9]*|"") fail "--tail must be a positive integer" ;; esac
[ "$startup_timeout" -gt 0 ] || fail "--timeout must be a positive integer"
[ "$tail_lines" -gt 0 ] || fail "--tail must be a positive integer"

control_windows=$(to_windows_path "$CONTROL_SCRIPT")
runtime_args=
if [ -n "$runtime_dir" ]; then
  runtime_windows=$(to_windows_path "$runtime_dir")
  runtime_args="-RuntimeDirectory"
fi

invoke_control() {
  action=$1
  shift
  if [ -n "$runtime_args" ]; then
    powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass \
      -File "$control_windows" -Action "$action" \
      -RuntimeDirectory "$runtime_windows" "$@"
  else
    powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass \
      -File "$control_windows" -Action "$action" "$@"
  fi
}

invoke_start_background() {
  action=$1
  shift
  startup_output=$(mktemp "${TMPDIR:-/tmp}/device-hub-windows-start.XXXXXX")
  if [ -n "$runtime_args" ]; then
    nohup powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass \
      -File "$control_windows" -Action "$action" \
      -RuntimeDirectory "$runtime_windows" "$@" \
      >"$startup_output" 2>&1 </dev/null &
  else
    nohup powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass \
      -File "$control_windows" -Action "$action" "$@" \
      >"$startup_output" 2>&1 </dev/null &
  fi
  controller_pid=$!
  controller_status=0
  deadline=$(( $(date +%s) + startup_timeout + 5 ))

  while [ "$(date +%s)" -lt "$deadline" ]; do
    if grep -Eq '^Device Hub (started|is already running)' "$startup_output"; then
      sed -n '1,20p' "$startup_output"
      rm -f "$startup_output"
      return 0
    fi
    if ! kill -0 "$controller_pid" 2>/dev/null; then
      wait "$controller_pid" || controller_status=$?
      sed -n '1,80p' "$startup_output" >&2
      rm -f "$startup_output"
      return "$controller_status"
    fi
    sleep 0.2
  done

  printf 'error: timed out waiting for the Windows launcher\n' >&2
  sed -n '1,80p' "$startup_output" >&2
  rm -f "$startup_output"
  return 1
}

case "$command_name" in
  start|foreground)
    if [ ! -f "$server_path" ] && ! printf '%s' "$server_path" | grep -Eq '^[A-Za-z]:\\|^\\\\'; then
      fail "Windows server not found: $server_path (run tools/build-windows-from-linux.sh first)"
    fi
    if [ ! -f "$config_path" ] && ! printf '%s' "$config_path" | grep -Eq '^[A-Za-z]:\\|^\\\\'; then
      fail "config not found: $config_path"
    fi
    server_windows=$(to_windows_path "$server_path")
    config_windows=$(to_windows_path "$config_path")
    action=Start
    [ "$command_name" = foreground ] && action=Foreground
    if [ "$command_name" = foreground ]; then
      invoke_control "$action" \
        -ServerPath "$server_windows" \
        -ConfigPath "$config_windows" \
        -BindAddress "$bind_address" \
        -StartupTimeoutSeconds "$startup_timeout"
    else
      invoke_start_background "$action" \
        -ServerPath "$server_windows" \
        -ConfigPath "$config_windows" \
        -BindAddress "$bind_address" \
        -StartupTimeoutSeconds "$startup_timeout"
    fi
    ;;
  stop)
    invoke_control Stop
    ;;
  status)
    invoke_control Status
    ;;
  restart)
    if [ ! -f "$server_path" ] && ! printf '%s' "$server_path" | grep -Eq '^[A-Za-z]:\\|^\\\\'; then
      fail "Windows server not found: $server_path (run tools/build-windows-from-linux.sh first)"
    fi
    if [ ! -f "$config_path" ] && ! printf '%s' "$config_path" | grep -Eq '^[A-Za-z]:\\|^\\\\'; then
      fail "config not found: $config_path"
    fi
    invoke_control Stop
    server_windows=$(to_windows_path "$server_path")
    config_windows=$(to_windows_path "$config_path")
    invoke_start_background Start \
      -ServerPath "$server_windows" \
      -ConfigPath "$config_windows" \
      -BindAddress "$bind_address" \
      -StartupTimeoutSeconds "$startup_timeout"
    ;;
  logs)
    if [ "$follow" = true ]; then
      invoke_control Logs -Tail "$tail_lines" -Follow
    else
      invoke_control Logs -Tail "$tail_lines"
    fi
    ;;
esac
