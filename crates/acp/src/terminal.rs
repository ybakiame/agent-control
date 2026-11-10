use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

pub struct TerminalEntry {
    pub child: Child,
    pub output: Arc<Mutex<String>>,
    pub byte_limit: usize,
}

#[derive(Default)]
pub struct TerminalManager {
    entries: Mutex<HashMap<String, TerminalEntry>>,
}

impl TerminalManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn create(
        &self,
        command: &str,
        args: &[String],
        env: &[(String, String)],
        cwd: Option<&std::path::Path>,
        byte_limit: Option<u64>,
    ) -> Result<String> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .kill_on_drop(true);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn()?;
        let output = Arc::new(Mutex::new(String::new()));
        let limit = byte_limit.unwrap_or(1_048_576) as usize;

        if let Some(stdout) = child.stdout.take() {
            let sink = output.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut guard = sink.lock().await;
                            guard.push_str(&String::from_utf8_lossy(&buf[..n]));
                            if guard.len() > limit {
                                let excess = guard.len() - limit;
                                guard.drain(..excess);
                            }
                        }
                    }
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let sink = output.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut guard = sink.lock().await;
                            guard.push_str(&String::from_utf8_lossy(&buf[..n]));
                            if guard.len() > limit {
                                let excess = guard.len() - limit;
                                guard.drain(..excess);
                            }
                        }
                    }
                }
            });
        }

        let id = uuid::Uuid::new_v4().to_string();
        self.entries.lock().await.insert(
            id.clone(),
            TerminalEntry {
                child,
                output,
                byte_limit: limit,
            },
        );
        Ok(id)
    }

    pub async fn output(&self, id: &str) -> Result<(String, bool, Option<u32>)> {
        let mut entries = self.entries.lock().await;
        let entry = entries
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
        let text = entry.output.lock().await.clone();
        let exit_code = entry
            .child
            .try_wait()?
            .map(|s| s.code().unwrap_or(-1) as u32);
        Ok((text, false, exit_code))
    }

    pub async fn wait_exit(&self, id: &str) -> Result<Option<u32>> {
        let mut entries = self.entries.lock().await;
        let entry = entries
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
        let status = entry.child.wait().await?;
        Ok(Some(status.code().unwrap_or(-1) as u32))
    }

    pub async fn kill(&self, id: &str) -> Result<()> {
        let mut entries = self.entries.lock().await;
        if let Some(entry) = entries.get_mut(id) {
            entry.child.start_kill()?;
        }
        Ok(())
    }

    pub async fn release(&self, id: &str) -> Result<()> {
        self.entries.lock().await.remove(id);
        Ok(())
    }
}
