use std::io;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

pub const DEFAULT_READ_CHUNK: usize = 8 * 1024;

#[derive(Debug, Error)]
pub enum SubprocessError {
    #[error("failed to spawn `{label}`: {source}")]
    Spawn { label: String, source: io::Error },
    #[error("failed waiting on `{label}`: {source}")]
    Wait { label: String, source: io::Error },
}

#[derive(Debug)]
pub struct BoundedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
}

pub async fn run_with_bounded_output(
    label: &str,
    mut command: Command,
    timeout: Duration,
    stdout_cap: usize,
    stderr_cap: usize,
) -> Result<BoundedOutput, SubprocessError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn().map_err(|source| SubprocessError::Spawn {
        label: label.to_string(),
        source,
    })?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_task = tokio::spawn(read_bounded(stdout, stdout_cap));
    let stderr_task = tokio::spawn(read_bounded(stderr, stderr_cap));

    let wait_result = tokio::time::timeout(timeout, child.wait()).await;
    if wait_result.is_err() {
        let _ = child.kill().await;
    }

    let (stdout_result, stderr_result) = tokio::join!(stdout_task, stderr_task);
    let (stdout, stdout_truncated) = stdout_result
        .map_err(|source| SubprocessError::Wait {
            label: label.to_string(),
            source: io::Error::other(source),
        })?
        .map_err(|source| SubprocessError::Wait {
            label: label.to_string(),
            source,
        })?;
    let (stderr, stderr_truncated) = stderr_result
        .map_err(|source| SubprocessError::Wait {
            label: label.to_string(),
            source: io::Error::other(source),
        })?
        .map_err(|source| SubprocessError::Wait {
            label: label.to_string(),
            source,
        })?;

    let status = match wait_result {
        Ok(Ok(status)) => status,
        Ok(Err(source)) => {
            return Err(SubprocessError::Wait {
                label: label.to_string(),
                source,
            });
        }
        Err(_) => fake_failure_status(),
    };

    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        timed_out: wait_result.is_err(),
    })
}

async fn read_bounded<R>(pipe: Option<R>, cap: usize) -> io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let Some(mut pipe) = pipe else {
        return Ok((Vec::new(), false));
    };

    let mut kept = Vec::with_capacity(cap.min(DEFAULT_READ_CHUNK));
    let mut truncated = false;
    let mut buf = [0u8; DEFAULT_READ_CHUNK];

    loop {
        let n = pipe.read(&mut buf).await?;
        if n == 0 {
            break;
        }

        if kept.len() < cap {
            let remaining = cap - kept.len();
            let take = remaining.min(n);
            kept.extend_from_slice(&buf[..take]);
            if take < n {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }

    Ok((kept, truncated))
}

#[cfg(unix)]
fn fake_failure_status() -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(0xff)
}

#[cfg(windows)]
fn fake_failure_status() -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_helper_caps_stdout_and_does_not_deadlock() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("yes x | head -c 131072");

        let out = run_with_bounded_output(
            "yes-head",
            command,
            Duration::from_secs(2),
            32 * 1024,
            32 * 1024,
        )
        .await
        .expect("run succeeds");

        assert!(out.stdout_truncated, "stdout should be marked truncated");
        assert!(out.stdout.len() <= 32 * 1024, "kept bytes within cap");
        assert!(out.status.success());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_helper_drains_simultaneous_stdout_and_stderr() {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("yes x | head -c 65536 & yes y | head -c 65536 1>&2; wait");

        let out = run_with_bounded_output(
            "both",
            command,
            Duration::from_secs(2),
            16 * 1024,
            16 * 1024,
        )
        .await
        .expect("run succeeds");

        assert!(out.stdout_truncated);
        assert!(out.stderr_truncated);
        assert!(out.stdout.len() <= 16 * 1024);
        assert!(out.stderr.len() <= 16 * 1024);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_helper_reports_timeout() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("sleep 5");

        let out = run_with_bounded_output(
            "sleep",
            command,
            Duration::from_millis(200),
            1024,
            1024,
        )
        .await
        .expect("run resolves with timed_out=true");

        assert!(out.timed_out);
    }
}
