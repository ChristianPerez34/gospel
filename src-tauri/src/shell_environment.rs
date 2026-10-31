use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

pub fn initialize() {
    let default_shell = if cfg!(target_os = "macos") {
        "/bin/zsh"
    } else {
        "/bin/sh"
    };
    let shell = std::env::var_os("SHELL")
        .filter(|shell| Path::new(shell).is_absolute())
        .unwrap_or_else(|| default_shell.into());
    let mut command = Command::new(shell);
    // Interactive login mode loads both profile and rc paths (Homebrew, Bun, nvm).
    // Disable job control (+m) so interactive shells in background process groups
    // are not suspended by SIGTTIN/SIGTTOU on terminal-attached runs.
    command
        .args(["+m", "-ilc", "printf '\\0GOSPEL_PATH\\0%s\\0' \"$PATH\""])
        .current_dir(
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| "/".into()),
        );

    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
        .and_then(|runtime| runtime.block_on(read_path(command, Duration::from_secs(5))));

    // The temporary runtime has been dropped; startup is still single-threaded.
    match result {
        Ok(path) => std::env::set_var("PATH", path),
        Err(error) => {
            eprintln!("Warning: could not recover shell PATH; keeping inherited PATH: {error}")
        }
    }
}

async fn read_path(command: Command, timeout: Duration) -> Result<OsString, String> {
    // Background jobs can keep pipes open even after the shell exits.
    let output = tokio::time::timeout(
        timeout + Duration::from_secs(1),
        crate::subprocess_output::run_with_bounded_output(
            "login shell PATH",
            command,
            timeout,
            64 * 1024,
            1024,
        ),
    )
    .await
    .map_err(|_| "login shell timed out".to_string())?
    .map_err(|error| error.to_string())?;
    if output.timed_out {
        return Err("login shell timed out".into());
    }
    if !output.status.success() || output.stdout_truncated {
        return Err("login shell failed or exceeded its output limit".into());
    }

    // Shell startup files may print banners; only accept the marked PATH field.
    let path = output
        .stdout
        .split(|byte| *byte == 0)
        .skip_while(|field| *field != b"GOSPEL_PATH")
        .nth(1)
        .filter(|path| !path.is_empty())
        .ok_or("login shell did not report a non-empty PATH")?;
    Ok(OsString::from_vec(path.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(target_os = "macos")]
    use std::process::Command as StdCommand;

    #[test]
    #[cfg(target_os = "macos")]
    fn startup_recovers_tools_from_login_and_interactive_shell_config() {
        if std::env::var_os("GOSPEL_PATH_TEST_CHILD").is_some() {
            initialize();
            for tool in ["bun", "node"] {
                let output = StdCommand::new(tool).arg("--version").output().unwrap();
                assert!(output.status.success());
                assert_eq!(output.stdout, b"fixture-version\n");
            }
            return;
        }

        let home = tempfile::tempdir().unwrap();
        for (config, tool) in [(".zprofile", "node"), (".zshrc", "bun")] {
            let bin = home.path().join(format!("{tool} bin"));
            std::fs::create_dir(&bin).unwrap();
            let program = bin.join(tool);
            std::fs::write(&program, "#!/bin/sh\nprintf 'fixture-version\\n'\n").unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(
                home.path().join(config),
                format!(
                    "export PATH=\"{}:$PATH\"\nprintf 'startup noise\\n'\n",
                    bin.display()
                ),
            )
            .unwrap();
        }

        // A child process isolates PATH mutation from parallel tests.
        let output = StdCommand::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "shell_environment::tests::startup_recovers_tools_from_login_and_interactive_shell_config",
                "--nocapture",
            ])
            .env("GOSPEL_PATH_TEST_CHILD", "1")
            .env("HOME", home.path())
            .env("ZDOTDIR", home.path())
            .env("SHELL", "/bin/zsh")
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn rejects_failed_empty_missing_and_truncated_shell_output() {
        for script in [
            "printf '\\0GOSPEL_PATH\\0/bin\\0'; exit 1",
            "printf '\\0GOSPEL_PATH\\0\\0'",
            "printf 'just a banner'",
            "printf '\\0GOSPEL_PATH\\0/bin\\0'; /usr/bin/yes x | /usr/bin/head -c 70000",
        ] {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            assert!(read_path(command, Duration::from_secs(2)).await.is_err());
        }
    }

    #[tokio::test]
    async fn preserves_path_bytes_and_ignores_banners() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "printf 'banner\\n\\0GOSPEL_PATH\\0/has spaces:/non-utf8-\\377\\0trailer'",
        ]);
        let path = read_path(command, Duration::from_secs(2)).await.unwrap();
        assert_eq!(path.into_vec(), b"/has spaces:/non-utf8-\xff");
    }

    #[tokio::test]
    async fn shell_startup_has_a_timeout() {
        for script in ["sleep 10", "sleep 2 &"] {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            let started = std::time::Instant::now();
            let error = read_path(command, Duration::from_millis(100))
                .await
                .unwrap_err();
            assert_eq!(error, "login shell timed out");
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    }
}
