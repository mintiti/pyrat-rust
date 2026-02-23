use std::process::{Child, Command, Stdio};

use tracing::info;

use super::config::BotConfig;

/// Error returned when bot launching fails.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("failed to spawn bot '{agent_id}': {source}")]
    SpawnFailed {
        agent_id: String,
        source: std::io::Error,
    },
}

/// RAII guard for spawned bot processes. Kills all children on drop.
#[derive(Debug)]
pub struct BotProcesses {
    children: Vec<(String, Child)>,
}

impl BotProcesses {
    /// Kill all child processes. Idempotent — safe to call multiple times.
    pub fn kill_all(&mut self) {
        for (agent_id, child) in &mut self.children {
            if let Err(e) = child.kill() {
                // Already dead is fine (InvalidInput on Unix).
                if e.kind() != std::io::ErrorKind::InvalidInput {
                    tracing::debug!(agent_id, error = %e, "kill failed (likely already exited)");
                }
            }
            let _ = child.wait();
        }
    }

    pub fn len(&self) -> usize {
        self.children.len()
    }

    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

impl Drop for BotProcesses {
    fn drop(&mut self) {
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
                let mut guard = BotProcesses { children };
                guard.kill_all();
                return Err(LaunchError::SpawnFailed {
                    agent_id: bot.agent_id.clone(),
                    source,
                });
            },
        };

        info!(agent_id = bot.agent_id, "spawned bot process");
        children.push((bot.agent_id.clone(), child));
    }

    Ok(BotProcesses { children })
}

/// Spawn a single bot process via a platform shell.
fn spawn_bot(bot: &BotConfig, port: u16) -> std::io::Result<Child> {
    let mut cmd = shell_command(&bot.run_command);
    cmd.current_dir(&bot.working_dir)
        .env("PYRAT_AGENT_ID", &bot.agent_id)
        .env("PYRAT_HOST_PORT", port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    cmd.spawn()
}

/// Build a `Command` that runs `run_command` through the platform shell.
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
        let cmd = idle_command();
        // Bot A is valid, Bot B has a nonexistent working directory which
        // makes Command::spawn() itself fail (not just the inner command).
        let mut bad = bot("b", &cmd);
        bad.working_dir = PathBuf::from("/nonexistent_dir_that_wont_exist");

        let bots = vec![bot("a", &cmd), bad];
        let result = launch_bots(&bots, 9999);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("b"));
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
        let pid = procs.children[0].1.id();
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
