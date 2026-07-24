use std::{
    env, fs,
    io::{self, Write},
    path::Path,
    process,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("adapter process fixture failed: {error}");
        process::exit(97);
    }
}

fn run() -> Result<(), String> {
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let scenario = executable
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "fixture executable has no UTF-8 file stem".to_owned())?;
    let args = env::args().skip(1).collect::<Vec<_>>();

    let scenario = resolve_scenario(scenario, &args)?;
    match scenario {
        "ssh-exec-success" => {
            require_args(&args, &["-i", "test_key", "root@board.local", "uname -a"])?;
            output(b"remote stdout", b"remote stderr", 0)
        }
        "ssh-exit-seven" => output(b"partial output", b"command failed", 7),
        "ssh-permission-denied" => output(b"", b"permission denied", 255),
        "ssh-reset-disconnect" => output(b"", b"Connection reset by peer", 255),
        "ssh-tail-lines" => output(b"line one\nline two\n", b"warn one\n", 0),
        "jlink-probe" => {
            let command_file = argument_after(&args, "-CommandFile")
                .ok_or_else(|| "missing -CommandFile argument".to_owned())?;
            let mut stdout = fs::read(command_file).map_err(|error| {
                format!("read command file {}: {error}", command_file.display())
            })?;
            stdout.extend_from_slice(b"probe ok\n");
            output(&stdout, b"", 0)
        }
        "jlink-failure" => output(b"attach failed\n", b"dap error\n", 1),
        "adb-invalid-utf8" => output(b"valid\nbad \xff\n", b"", 0),
        other => Err(format!("unknown fixture scenario `{other}`")),
    }
}

fn resolve_scenario<'a>(executable_name: &'a str, args: &[String]) -> Result<&'a str, String> {
    match executable_name {
        "ssh-process-fixture" => {
            let remote_command = args
                .last()
                .ok_or_else(|| "SSH fixture received no remote command".to_owned())?;
            if remote_command == "uname -a" {
                Ok("ssh-exec-success")
            } else if remote_command == "false" {
                Ok("ssh-exit-seven")
            } else if remote_command == "true" {
                Ok("ssh-permission-denied")
            } else if remote_command.contains("reboot") {
                Ok("ssh-reset-disconnect")
            } else if remote_command.starts_with("tail -F") {
                Ok("ssh-tail-lines")
            } else {
                Err(format!("unknown SSH fixture command `{remote_command}`"))
            }
        }
        "jlink-process-fixture" => {
            let command_file = argument_after(args, "-CommandFile")
                .ok_or_else(|| "missing -CommandFile argument".to_owned())?;
            let commands = fs::read_to_string(command_file)
                .map_err(|error| format!("read {}: {error}", command_file.display()))?;
            if commands.contains("ShowEmuList") {
                Ok("jlink-probe")
            } else if commands.lines().next() == Some("r") {
                Ok("jlink-failure")
            } else {
                Err(format!("unknown J-Link fixture commands: {commands:?}"))
            }
        }
        "adb-process-fixture" => Ok("adb-invalid-utf8"),
        scenario => Ok(scenario),
    }
}

fn require_args(actual: &[String], expected: &[&str]) -> Result<(), String> {
    let joined = actual.join(" ");
    for expected_value in expected {
        if !joined.contains(expected_value) {
            return Err(format!(
                "expected argument fragment {expected_value:?}; actual arguments: {actual:?}"
            ));
        }
    }
    Ok(())
}

fn argument_after<'a>(args: &'a [String], needle: &str) -> Option<&'a Path> {
    let index = args.iter().position(|argument| argument == needle)?;
    args.get(index + 1).map(Path::new)
}

fn output(stdout: &[u8], stderr: &[u8], exit_code: i32) -> Result<(), String> {
    write_and_flush(io::stdout(), stdout).map_err(|error| error.to_string())?;
    write_and_flush(io::stderr(), stderr).map_err(|error| error.to_string())?;
    process::exit(exit_code);
}

fn write_and_flush(mut writer: impl Write, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}
