//! Process execution for actions. Drain both pipes while the child is running,
//! bound captured output, and cancel the whole Unix process group.
use crate::action::ActionExecutionControl;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::Duration;

const OUTPUT_LIMIT: usize = 1024 * 1024;

fn prepare(command: &mut Command) {
    command.stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    // The child is the leader of the fresh group created in prepare().
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn check_cancel(child: &mut Child, control: Option<&ActionExecutionControl>) -> Result<(), String> {
    if control.is_some_and(ActionExecutionControl::is_cancelled) {
        terminate(child);
        Err("Action cancelled".into())
    } else {
        Ok(())
    }
}

pub fn status(
    command: Command,
    control: Option<&ActionExecutionControl>,
    context: &str,
) -> Result<ExitStatus, String> {
    status_with_pid(command, control, context).map(|(_, status)| status)
}

pub fn status_with_pid(
    mut command: Command,
    control: Option<&ActionExecutionControl>,
    context: &str,
) -> Result<(u32, ExitStatus), String> {
    if control.is_some_and(ActionExecutionControl::is_cancelled) {
        return Err("Action cancelled".into());
    }
    prepare(&mut command);
    let mut child = command
        .spawn()
        .map_err(|err| format!("Failed to start {context}: {err}"))?;
    loop {
        check_cancel(&mut child, control)?;
        match child.try_wait() {
            Ok(Some(status)) => return Ok((child.id(), status)),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => {
                terminate(&mut child);
                return Err(format!("Failed waiting for {context}: {err}"));
            }
        }
    }
}

#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // These calls change only the owned pipe's file status flags.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn drain(pipe: &mut impl Read, bytes: &mut Vec<u8>, truncated: &mut bool) -> io::Result<()> {
    let mut buffer = [0; 8192];
    // Bound work per poll so continuous output cannot starve cancellation.
    for _ in 0..32 {
        match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let keep = count.min(OUTPUT_LIMIT.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&buffer[..keep]);
                *truncated |= keep < count;
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

pub fn output(
    command: Command,
    control: Option<&ActionExecutionControl>,
    context: &str,
) -> Result<Output, String> {
    output_with_pid(command, control, context).map(|(_, output)| output)
}

pub fn detached(
    mut command: Command,
    control: Option<&ActionExecutionControl>,
) -> Result<u32, String> {
    if control.is_some_and(ActionExecutionControl::is_cancelled) {
        return Err("Action cancelled".into());
    }
    prepare(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to start program: {e}"))?;
    check_cancel(&mut child, control)?;
    let pid = child.id();
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

#[cfg(unix)]
pub fn output_with_pid(
    command: Command,
    control: Option<&ActionExecutionControl>,
    context: &str,
) -> Result<(u32, Output), String> {
    output_with_monitor(command, control, context, |_| Ok(()))
}

#[cfg(unix)]
pub(crate) fn output_with_monitor(
    mut command: Command,
    control: Option<&ActionExecutionControl>,
    context: &str,
    mut monitor: impl FnMut(u32) -> Result<(), String>,
) -> Result<(u32, Output), String> {
    if control.is_some_and(ActionExecutionControl::is_cancelled) {
        return Err("Action cancelled".into());
    }
    prepare(&mut command);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| format!("Failed to start {context}: {err}"))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    if let Err(err) = nonblocking(&stdout).and_then(|_| nonblocking(&stderr)) {
        terminate(&mut child);
        return Err(err.to_string());
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let (mut out_truncated, mut err_truncated) = (false, false);
    let status = loop {
        check_cancel(&mut child, control)?;
        if let Err(error) = monitor(child.id()) {
            terminate(&mut child);
            return Err(error);
        }
        let read = drain(&mut stdout, &mut out, &mut out_truncated)
            .and_then(|_| drain(&mut stderr, &mut err, &mut err_truncated));
        if let Err(error) = read {
            terminate(&mut child);
            return Err(error.to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // Read remaining buffered data without waiting for background
                // descendants which might still hold a copy of the write end.
                drain(&mut stdout, &mut out, &mut out_truncated).map_err(|e| e.to_string())?;
                drain(&mut stderr, &mut err, &mut err_truncated).map_err(|e| e.to_string())?;
                break status;
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                terminate(&mut child);
                return Err(error.to_string());
            }
        }
    };
    if out_truncated {
        out.extend_from_slice(b"\n[stdout truncated at 1 MiB]\n");
    }
    if err_truncated {
        err.extend_from_slice(b"\n[stderr truncated at 1 MiB]\n");
    }
    Ok((
        child.id(),
        Output {
            status,
            stdout: out,
            stderr: err,
        },
    ))
}

#[cfg(not(unix))]
pub fn output_with_pid(
    mut command: Command,
    control: Option<&ActionExecutionControl>,
    context: &str,
) -> Result<(u32, Output), String> {
    // Files avoid pipe deadlocks on platforms without nonblocking Unix pipes.
    let mut stdout = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut stderr = tempfile::tempfile().map_err(|e| e.to_string())?;
    command.stdout(stdout.try_clone().map_err(|e| e.to_string())?);
    command.stderr(stderr.try_clone().map_err(|e| e.to_string())?);
    let (pid, status) = status_with_pid(command, control, context)?;
    use std::io::{Seek, SeekFrom};
    stdout.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    stderr.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    stdout
        .take(OUTPUT_LIMIT as u64)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    stderr
        .take(OUTPUT_LIMIT as u64)
        .read_to_end(&mut err)
        .map_err(|e| e.to_string())?;
    Ok((
        pid,
        Output {
            status,
            stdout: out,
            stderr: err,
        },
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Instant;

    fn shell(script: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd
    }

    #[test]
    fn monitor_error_terminates_and_reaps_the_managed_child() {
        let mut pid = 0;
        let result = output_with_monitor(shell("sleep 30"), None, "monitor test", |child_pid| {
            pid = child_pid;
            Err("Monitor failed".into())
        });
        assert_eq!(result.unwrap_err(), "Monitor failed");
        assert!(pid > 0);
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn drains_large_stdout_and_stderr_without_deadlock() {
        let result = output(
            shell("head -c 2097152 /dev/zero; head -c 2097152 /dev/zero >&2"),
            None,
            "test",
        )
        .unwrap();
        assert!(result.status.success());
        assert!(result.stdout.len() > OUTPUT_LIMIT && result.stdout.len() < OUTPUT_LIMIT + 100);
        assert!(result.stderr.len() > OUTPUT_LIMIT && result.stderr.len() < OUTPUT_LIMIT + 100);
    }

    #[test]
    fn cancellation_kills_shell_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("must-not-exist");
        let mut cmd = shell("(sleep 0.4; touch \"$1\") & wait");
        cmd.arg("test").arg(&marker);
        let control = ActionExecutionControl::new();
        let signal = control.clone();
        let cancel = thread::spawn(move || {
            thread::sleep(Duration::from_millis(80));
            signal.cancel();
        });
        assert_eq!(
            output(cmd, Some(&control), "test").unwrap_err(),
            "Action cancelled"
        );
        cancel.join().unwrap();
        thread::sleep(Duration::from_millis(450));
        assert!(!marker.exists());
    }

    #[test]
    fn background_descendant_does_not_hold_output_open() {
        let start = Instant::now();
        let result = output(shell("sleep 1 & printf done"), None, "test").unwrap();
        assert_eq!(result.stdout, b"done");
        assert!(start.elapsed() < Duration::from_millis(800));
    }
}
