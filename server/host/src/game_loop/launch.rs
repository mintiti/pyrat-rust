use std::collections::HashSet;
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tracing::{debug, info, warn};

use super::config::BotConfig;

/// Error returned when bot launching fails.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("failed to spawn bot '{agent_id}' (command: {run_command}): {source}")]
    SpawnFailed {
        agent_id: String,
        run_command: String,
        source: std::io::Error,
    },
}

/// Exit information for a bot process.
#[derive(Debug)]
pub struct BotExitInfo {
    pub agent_id: String,
    pub status: Option<ExitStatus>,
}

/// Format an `ExitStatus` for display, including signal info on Unix.
fn describe_exit(status: &ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("signal {signal}");
        }
    }
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => "unknown exit status".to_string(),
    }
}

/// RAII guard for spawned bot processes. Kills all children on drop.
///
/// Supports background exit monitoring (`start_exit_monitor`) and stderr
/// handle extraction (`take_stderr_handles`) for observability.
#[derive(Debug)]
#[must_use = "dropping BotProcesses immediately kills all spawned bots"]
pub struct BotProcesses {
    children: Arc<Mutex<Vec<(String, Child)>>>,
    game_over: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    monitor_handle: Option<std::thread::JoinHandle<()>>,
}

impl BotProcesses {
    fn new(children: Vec<(String, Child)>) -> Self {
        Self {
            children: Arc::new(Mutex::new(children)),
            game_over: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            monitor_handle: None,
        }
    }

    /// Kill all child processes. Idempotent — safe to call multiple times.
    pub fn kill_all(&mut self) {
        let mut children = self.children.lock().unwrap();
        for (agent_id, child) in children.iter_mut() {
            if let Err(e) = child.kill() {
                debug!(agent_id, error = %e, "kill failed (likely already exited)");
            }
            let _ = child.wait();
        }
    }

    pub fn len(&self) -> usize {
        self.children.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.children.lock().unwrap().is_empty()
    }

    /// Returns the PID of the child at the given index.
    pub fn pid(&self, index: usize) -> Option<u32> {
        self.children
            .lock()
            .unwrap()
            .get(index)
            .map(|(_, child)| child.id())
    }

    /// Returns exit info for the first child that has exited, or `None` if all
    /// are still running.
    pub fn try_exited(&self) -> Option<BotExitInfo> {
        let mut children = self.children.lock().unwrap();
        for (agent_id, child) in children.iter_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Some(BotExitInfo {
                        agent_id: agent_id.clone(),
                        status: Some(status),
                    });
                },
                Err(_) => {
                    return Some(BotExitInfo {
                        agent_id: agent_id.clone(),
                        status: None,
                    });
                },
                Ok(None) => {},
            }
        }
        None
    }

    /// Drain `ChildStderr` handles from each child process.
    ///
    /// Returns `(agent_id, stderr)` pairs. Each handle can only be taken once;
    /// subsequent calls return an empty vec.
    pub fn take_stderr_handles(&mut self) -> Vec<(String, ChildStderr)> {
        let mut children = self.children.lock().unwrap();
        children
            .iter_mut()
            .filter_map(|(agent_id, child)| {
                child.stderr.take().map(|stderr| (agent_id.clone(), stderr))
            })
            .collect()
    }

    /// Start a background thread that polls for bot process exits.
    ///
    /// Exits before `mark_game_over()` are logged as warnings (unexpected).
    /// Exits after are logged as debug (expected shutdown).
    pub fn start_exit_monitor(&mut self, span: tracing::Span) {
        let children = Arc::clone(&self.children);
        let game_over = Arc::clone(&self.game_over);
        let stop = Arc::clone(&self.stop);

        let handle = std::thread::spawn(move || {
            let _guard = span.enter();
            let mut seen: HashSet<String> = HashSet::new();

            while !stop.load(Ordering::Relaxed) {
                {
                    let mut lock = children.lock().unwrap();
                    for (agent_id, child) in lock.iter_mut() {
                        if seen.contains(agent_id) {
                            continue;
                        }
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                let desc = describe_exit(&status);
                                if game_over.load(Ordering::Relaxed) {
                                    debug!(agent_id, status = %desc, "bot process exited");
                                } else {
                                    warn!(agent_id, status = %desc, "bot process exited unexpectedly");
                                }
                                seen.insert(agent_id.clone());
                            },
                            Err(e) => {
                                warn!(agent_id, error = %e, "failed to check bot process status");
                                seen.insert(agent_id.clone());
                            },
                            Ok(None) => {},
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });

        self.monitor_handle = Some(handle);
    }

    /// Mark the game as finished. Subsequent process exits are logged at debug
    /// level instead of warn.
    pub fn mark_game_over(&self) {
        self.game_over.store(true, Ordering::Relaxed);
    }
}

impl Drop for BotProcesses {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.monitor_handle.take() {
            let _ = handle.join();
        }
        self.kill_all();
    }
}

/// Spawn bot subprocesses for each config entry.
///
/// - Configs with an empty `run_command` are skipped (manual start).
/// - Duplicate `agent_id` entries are spawned once (hivemind dedup).
/// - On any spawn failure, all already-spawned processes are killed before
///   returning the error.
///
/// The spawned process receives two env vars:
/// - `PYRAT_AGENT_ID` — the bot's agent identifier
/// - `PYRAT_HOST_PORT` — the TCP port to connect to
///
/// **Note:** A successful return means processes were spawned, not that the
/// bots are running or connected. Shell wrapping (`sh -c ...`) means a bad
/// inner command still spawns `sh` successfully. The caller detects dead bots
/// via connection timeout during the setup phase.
pub fn launch_bots(bots: &[BotConfig], port: u16) -> Result<BotProcesses, LaunchError> {
    let mut children: Vec<(String, Child)> = Vec::new();

    for bot in bots {
        // Skip empty run_command (manual start).
        if bot.run_command.is_empty() {
            info!(agent_id = bot.agent_id, "empty run_command, skipping spawn");
            continue;
        }

        // Hivemind dedup: skip if we already spawned this agent_id.
        if children.iter().any(|(id, _)| id == &bot.agent_id) {
            info!(
                agent_id = bot.agent_id,
                "duplicate agent_id, skipping (hivemind)"
            );
            continue;
        }

        let child = match spawn_bot(bot, port) {
            Ok(child) => child,
            Err(source) => {
                // Roll back: kill everything we already spawned.
                let mut guard = BotProcesses::new(children);
                guard.kill_all();
                return Err(LaunchError::SpawnFailed {
                    agent_id: bot.agent_id.clone(),
                    run_command: bot.run_command.clone(),
                    source,
                });
            },
        };

        info!(agent_id = bot.agent_id, "spawned bot process");
        children.push((bot.agent_id.clone(), child));
    }

    Ok(BotProcesses::new(children))
}

/// Spawn a single bot process via a platform shell.
fn spawn_bot(bot: &BotConfig, port: u16) -> std::io::Result<Child> {
    let mut cmd = shell_command(&bot.run_command);
    cmd.current_dir(&bot.working_dir)
        .env("PYRAT_AGENT_ID", &bot.agent_id)
        .env("PYRAT_HOST_PORT", port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    cmd.spawn()
}

/// Build a `Command` that runs `run_command` through the platform shell.
///
/// NOTE: `BotProcesses::kill_all` sends SIGKILL to the direct child (`sh`),
/// not the process tree. If the bot spawns its own subprocesses, they become
/// orphans. Process groups (setsid / CREATE_NEW_PROCESS_GROUP) would fix this
/// but aren't worth the complexity yet.
#[cfg(unix)]
fn shell_command(run_command: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", run_command]);
    cmd
}

#[cfg(windows)]
fn shell_command(run_command: &str) -> Command {
    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/c", run_command]);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn idle_command() -> String {
        if cfg!(unix) {
            "sleep 10".to_string()
        } else {
            "timeout /t 10 /nobreak >nul".to_string()
        }
    }

    fn bot(agent_id: &str, run_command: &str) -> BotConfig {
        BotConfig {
            run_command: run_command.to_string(),
            working_dir: PathBuf::from("."),
            agent_id: agent_id.to_string(),
        }
    }

    #[test]
    fn empty_run_command_is_skipped() {
        let bots = vec![bot("a", "")];
        let procs = launch_bots(&bots, 9999).unwrap();
        assert!(procs.is_empty());
    }

    #[test]
    fn hivemind_dedup() {
        let cmd = idle_command();
        let bots = vec![bot("same", &cmd), bot("same", &cmd)];
        let procs = launch_bots(&bots, 9999).unwrap();
        assert_eq!(procs.len(), 1);
    }

    #[test]
    fn mixed_empty_and_real() {
        let cmd = idle_command();
        let bots = vec![bot("manual", ""), bot("real", &cmd)];
        let procs = launch_bots(&bots, 9999).unwrap();
        assert_eq!(procs.len(), 1);
    }

    #[test]
    fn spawn_failure_rolls_back() {
        let pid_file =
            std::env::temp_dir().join(format!("pyrat_test_rollback_{}", std::process::id()));
        let _ = std::fs::remove_file(&pid_file);

        // Bot A writes its PID then sleeps. Bot B has a bad working dir.
        #[cfg(unix)]
        let cmd_a = format!("echo $$ > {} && sleep 10", pid_file.display());
        #[cfg(not(unix))]
        let cmd_a = idle_command();

        let mut bad = bot("b", &idle_command());
        bad.working_dir = PathBuf::from("/nonexistent_dir_that_wont_exist");

        let bots = vec![bot("a", &cmd_a), bad];
        let result = launch_bots(&bots, 9999);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("b"));

        // Verify bot A was killed during rollback.
        // The shell may or may not have written its PID before SIGKILL landed —
        // on fast machines, rollback outraces sh startup. Both outcomes are correct:
        // PID written + process dead = rollback killed a running bot.
        // PID not written = rollback killed before first instruction.
        #[cfg(unix)]
        {
            if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
                let pid = pid_str.trim();
                if !pid.is_empty() {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let status = Command::new("kill").args(["-0", pid]).status().unwrap();
                    assert!(
                        !status.success(),
                        "bot A (pid {pid}) should be dead after rollback"
                    );
                }
            }
            let _ = std::fs::remove_file(&pid_file);
        }
    }

    #[test]
    fn kill_all_idempotent() {
        let cmd = idle_command();
        let bots = vec![bot("a", &cmd)];
        let mut procs = launch_bots(&bots, 9999).unwrap();
        procs.kill_all();
        procs.kill_all(); // should not panic
    }

    #[test]
    fn drop_kills_process() {
        let cmd = idle_command();
        let bots = vec![bot("a", &cmd)];
        let procs = launch_bots(&bots, 9999).unwrap();

        // Grab the pid before dropping.
        let pid = procs.pid(0).unwrap();
        drop(procs);

        // Give the OS a moment to clean up.
        std::thread::sleep(std::time::Duration::from_millis(50));

        // On Unix, try to signal the process — should fail with "no such process".
        #[cfg(unix)]
        {
            use std::process::Command;
            let status = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .unwrap();
            assert!(!status.success(), "process {pid} should be dead after drop");
        }
    }
}
