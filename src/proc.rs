//! Bounded child processes. The caller owns the child for its whole life: one
//! deadline for the operation, kill the process group on expiry, then reap.

use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// Hard ceiling for any child `cos` starts. Lower per call, never higher.
pub const CEILING: Duration = Duration::from_secs(300);

pub struct Output {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Run to completion or kill at `deadline`. Never waits past `CEILING`.
pub fn run(cmd: &mut Command, deadline: Duration) -> Result<Output> {
    let deadline = deadline.min(CEILING);
    let mut child = spawn_grouped(cmd.stdout(Stdio::piped()).stderr(Stdio::piped()))?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let status = wait_until(&mut child, Instant::now() + deadline)?;
    Ok(Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// Spawn with stdout delivered line by line; `on_line` returning `false` stops
/// reading early. The child is killed and reaped on deadline, early stop, or error.
pub fn stream_lines(
    cmd: &mut Command,
    deadline: Duration,
    mut on_line: impl FnMut(&str) -> Result<bool>,
) -> Result<(ExitStatus, String)> {
    let until = Instant::now() + deadline.min(CEILING);
    let mut child = spawn_grouped(cmd.stdout(Stdio::piped()).stderr(Stdio::piped()))?;
    let err = drain(child.stderr.take());
    let stdout = child.stdout.take().context("child stdout not piped")?;
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            kill_and_reap(&mut child);
            bail!("timed out after {}s", deadline.min(CEILING).as_secs());
        }
        match rx.recv_timeout(left) {
            Ok(line) => match on_line(&line) {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => {
                    kill_and_reap(&mut child);
                    return Err(e);
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = wait_until(&mut child, until)?;
    Ok((status, err.join().unwrap_or_default()))
}

fn spawn_grouped(cmd: &mut Command) -> Result<Child> {
    // Own process group, so a kill reaches the MCP servers `claude` starts too.
    cmd.stdin(Stdio::null()).process_group(0);
    cmd.spawn()
        .with_context(|| format!("spawn {:?}", cmd.get_program()))
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_string(&mut buf);
        }
        buf
    })
}

fn wait_until(child: &mut Child, until: Instant) -> Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= until {
            kill_and_reap(child);
            bail!("timed out waiting for child exit");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn kill_and_reap(child: &mut Child) {
    let pgid = child.id() as libc::pid_t;
    // SAFETY: signalling a process group we created; no memory is touched.
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}
