use std::{
    env, fs,
    io::{self, Write},
    path::Path,
    process, thread,
    time::Duration,
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FixtureSpec {
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
    #[serde(default)]
    exit_code: i32,
    #[serde(default)]
    delay_ms: u64,
    #[serde(default)]
    expected_args: Vec<String>,
    #[serde(default)]
    conditional_outputs: Vec<ConditionalOutput>,
    repeat_stdout: Option<RepeatedOutput>,
    #[serde(default)]
    append_command_file: bool,
}

#[derive(Debug, Deserialize)]
struct ConditionalOutput {
    arg_contains: String,
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
}

#[derive(Debug, Deserialize)]
struct RepeatedOutput {
    prefix: String,
    start: usize,
    end: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("device-hub test fixture failed: {error}");
        process::exit(97);
    }
}

fn run() -> Result<(), String> {
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let spec_path = fixture_spec_path(&executable);
    let spec: FixtureSpec = serde_json::from_slice(
        &fs::read(&spec_path).map_err(|error| format!("read {}: {error}", spec_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", spec_path.display()))?;
    let args = env::args().skip(1).collect::<Vec<_>>();

    assert_expected_args(&spec.expected_args, &args)?;

    let mut stdout = spec.stdout;
    let mut stderr = spec.stderr;
    let joined_args = args.join(" ");
    for output in spec.conditional_outputs {
        if joined_args.contains(&output.arg_contains) {
            stdout.push_str(&output.stdout);
            stderr.push_str(&output.stderr);
        }
    }
    if let Some(repeated) = spec.repeat_stdout {
        for index in repeated.start..=repeated.end {
            stdout.push_str(&repeated.prefix);
            stdout.push_str(&index.to_string());
            stdout.push('\n');
        }
    }
    if spec.append_command_file {
        let path = argument_after(&args, "-CommandFile")
            .ok_or_else(|| "missing -CommandFile argument".to_owned())?;
        stdout.push_str(
            &fs::read_to_string(path)
                .map_err(|error| format!("read command file {path}: {error}"))?,
        );
    }

    write_and_flush(io::stdout(), stdout.as_bytes()).map_err(|error| error.to_string())?;
    write_and_flush(io::stderr(), stderr.as_bytes()).map_err(|error| error.to_string())?;
    if spec.delay_ms > 0 {
        thread::sleep(Duration::from_millis(spec.delay_ms));
    }
    process::exit(spec.exit_code);
}

fn fixture_spec_path(executable: &Path) -> std::path::PathBuf {
    executable.with_extension("json")
}

fn assert_expected_args(expected: &[String], actual: &[String]) -> Result<(), String> {
    if expected.is_empty() {
        return Ok(());
    }
    let mut cursor = 0;
    for expected_argument in expected {
        let Some(offset) = actual[cursor..]
            .iter()
            .position(|argument| argument == expected_argument)
        else {
            return Err(format!(
                "expected ordered argument {expected_argument:?}; actual arguments: {actual:?}"
            ));
        };
        cursor += offset + 1;
    }
    Ok(())
}

fn argument_after<'a>(args: &'a [String], needle: &str) -> Option<&'a str> {
    let index = args.iter().position(|argument| argument == needle)?;
    args.get(index + 1).map(String::as_str)
}

fn write_and_flush(mut writer: impl Write, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}
