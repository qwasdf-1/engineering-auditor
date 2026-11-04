//! The operator's test command, run in a scratch copy: a cleared
//! environment, a timeout that kills the whole process group, and no
//! network where unprivileged namespaces allow it (Linux `unshare`).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Variables a toolchain needs that carry no credentials.
const KEPT_ENVIRONMENT: [&str; 7] = ["PATH", "HOME", "USER", "LANG", "LC_ALL", "TMPDIR", "TERM"];
const OUTPUT_TAIL_BYTES: u64 = 2048;
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestCommand {
    pub argv: Vec<String>,
    pub timeout: Duration,
    /// Environment variables passed through besides the kept few.
    pub pass_env: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestRun {
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub network_isolated: bool,
    /// The last bytes of standard output and error, interleaved.
    pub output_tail: String,
    /// Why the command could not run at all.
    pub error: Option<String>,
}

impl TestRun {
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && self.error.is_none()
    }

    /// `exited with 1`, `timed out after 30 s`, `could not start: …`.
    pub fn outcome(&self) -> String {
        match (&self.error, self.timed_out, self.exit_code) {
            (Some(error), _, _) => format!("could not run: {error}"),
            (None, true, _) => format!("timed out after {} ms", self.duration_ms),
            (None, false, Some(code)) => format!("exited with {code}"),
            (None, false, None) => "was killed by a signal".to_string(),
        }
    }
}

pub(super) fn run(command: &TestCommand, directory: &Path, log: &Path) -> TestRun {
    let isolated = network_isolation();
    let mut record = TestRun {
        argv: command.argv.clone(),
        exit_code: None,
        timed_out: false,
        duration_ms: 0,
        network_isolated: isolated,
        output_tail: String::new(),
        error: None,
    };
    let started = Instant::now();
    let mut child = match spawn(command, directory, log, isolated) {
        Ok(child) => child,
        Err(error) => {
            record.error = Some(error);
            return record;
        }
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= command.timeout => {
                kill_group(&mut child);
                let _ = child.wait();
                record.timed_out = true;
                break None;
            }
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(error) => {
                record.error = Some(error.to_string());
                kill_group(&mut child);
                break None;
            }
        }
    };
    record.duration_ms = started.elapsed().as_millis() as u64;
    record.exit_code = status.and_then(|status| status.code());
    record.output_tail = output_tail(log);
    record
}

/// Starts the command in `directory` with a cleared environment, its own
/// process group, and both output streams in `log`.
fn spawn(
    command: &TestCommand,
    directory: &Path,
    log: &Path,
    isolated: bool,
) -> Result<Child, String> {
    let (program, arguments) = command
        .argv
        .split_first()
        .ok_or("the test command is empty")?;
    let mut process = if isolated {
        let mut process = Command::new("unshare");
        process.args(["--net", "--map-root-user", "--", program]);
        process
    } else {
        Command::new(program)
    };
    process
        .args(arguments)
        .current_dir(directory)
        .env_clear()
        .stdin(Stdio::null());
    for name in KEPT_ENVIRONMENT
        .iter()
        .copied()
        .chain(command.pass_env.iter().map(String::as_str))
    {
        if let Some(value) = std::env::var_os(name) {
            process.env(name, value);
        }
    }
    let stdout =
        File::create(log).map_err(|error| format!("cannot open the output log: {error}"))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("cannot open the output log: {error}"))?;
    process.stdout(stdout).stderr(stderr);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    process.spawn().map_err(|error| error.to_string())
}

/// Whether `unshare` can give a child its own empty network namespace
/// without privileges; probed once.
fn network_isolation() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        cfg!(target_os = "linux")
            && Command::new("unshare")
                .args(["--net", "--map-root-user", "true"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
    })
}

/// Kills the child and everything it started (its process group).
fn kill_group(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{}", child.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

fn output_tail(log: &Path) -> String {
    let Ok(mut file) = File::open(log) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(OUTPUT_TAIL_BYTES)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}
