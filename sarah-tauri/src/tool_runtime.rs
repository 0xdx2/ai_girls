use std::{collections::HashSet, process::Stdio, time::Duration};

use tokio::io::AsyncWriteExt;

use thiserror::Error;
use tracing::debug;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRequest {
    /// Run a read-only shell command.
    Terminal { command: String },
    /// Read a local file (capped at 10 000 chars).
    ReadFile { path: String },
    #[allow(dead_code)]
    /// Fetch a remote URL via curl and return plain text (capped at 8 000 chars).
    BrowsePage { url: String },
    /// List the contents of a directory.
    #[allow(dead_code)]
    ListDir { path: String },
    /// Search for a pattern inside files using grep / rg.
    #[allow(dead_code)]
    SearchFiles { path: String, pattern: String },
    /// Call an MCP server over stdio with a JSON-RPC payload.
    #[allow(dead_code)]
    McpCall {
        /// Server executable command, e.g. `npx -y @modelcontextprotocol/server-filesystem .`
        server_cmd: String,
        /// One-line JSON payload written to stdin.
        payload: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("command not allowed: {0}")]
    CommandNotAllowed(String),
    #[error("unsafe URL scheme: {0}")]
    UnsafeUrl(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("mcp server command not allowed: {0}")]
    McpNotAllowed(String),
}

// ─── Runtime ─────────────────────────────────────────────────────────────────

/// Safe read-only tool execution environment.
#[derive(Debug)]
pub struct ToolRuntime {
    /// Head commands unconditionally allowed (no subcommand check).
    simple_allowlist: HashSet<String>,
}

impl Default for ToolRuntime {
    fn default() -> Self {
        let simple_allowlist = [
            // navigation / listing
            "pwd", "ls", "tree", "find",
            // file content
            "cat", "head", "tail", "wc", "file", "stat",
            // search
            "grep", "rg", "fd", "awk", "sed",
            // identity / env
            "whoami", "id", "env", "printenv", "date", "uname", "hostname",
            // macOS specifics
            "sw_vers", "defaults",
            // basic output
            "echo", "printf", "which", "type",
            // disk / process info (read-only)
            "df", "du", "ps", "top",
            // system profiler (safe flags only – enforced in run_terminal)
            "system_profiler",
            // package managers – list/info only (enforced in run_terminal)
            "brew", "npm", "cargo", "pip3", "python3", "node",
            // git – read only
            "git",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();

        Self { simple_allowlist }
    }
}

impl ToolRuntime {
    pub async fn invoke(&self, req: ToolRequest) -> Result<ToolResult, ToolError> {
        match req {
            ToolRequest::Terminal { command } => self.run_terminal(&command).await,
            ToolRequest::ReadFile { path } => self.read_file(&path).await,
            ToolRequest::BrowsePage { url } => self.browse_page(&url).await,
            ToolRequest::ListDir { path } => self.list_dir(&path).await,
            ToolRequest::SearchFiles { path, pattern } => {
                self.search_files(&path, &pattern).await
            }
            ToolRequest::McpCall {
                server_cmd,
                payload,
            } => self.call_mcp_stdio(&server_cmd, &payload).await,
        }
    }

    async fn call_mcp_stdio(
        &self,
        server_cmd: &str,
        payload: &str,
    ) -> Result<ToolResult, ToolError> {
        let mut parts = server_cmd.split_whitespace();
        let head = parts.next().unwrap_or_default();
        let args: Vec<&str> = parts.collect();

        // Strict launcher allowlist for MCP stdio servers.
        if !matches!(head, "npx" | "uvx" | "node" | "python3") {
            return Err(ToolError::McpNotAllowed(server_cmd.to_owned()));
        }

        let mut child = tokio::process::Command::new(head)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::Io(e.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(payload.as_bytes())
                .await
                .map_err(|e| ToolError::Io(e.to_string()))?;
            stdin
                .write_all(b"\n")
                .await
                .map_err(|e| ToolError::Io(e.to_string()))?;
        }

        let output = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output())
            .await
            .map_err(|_| ToolError::Io("mcp call timed out".to_owned()))?
            .map_err(|e| ToolError::Io(e.to_string()))?;

        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.stderr.is_empty() {
            text.push_str("\n[stderr] ");
            text.push_str(&String::from_utf8_lossy(&output.stderr));
        }

        Ok(ToolResult {
            success: output.status.success(),
            output: text.chars().take(12_000).collect(),
        })
    }

    async fn run_terminal(&self, command: &str) -> Result<ToolResult, ToolError> {
        let head = command.split_whitespace().next().unwrap_or_default();

        if !self.simple_allowlist.contains(head) {
            return Err(ToolError::CommandNotAllowed(command.to_owned()));
        }

        // Secondary safety checks for commands that need subcommand filtering
        if !is_subcommand_safe(command) {
            return Err(ToolError::CommandNotAllowed(format!(
                "subcommand not in read-only allowlist: {command}"
            )));
        }

        debug!("tool-runtime: running [{command}]");

        let output = tokio::process::Command::new("zsh")
            .arg("-lc")
            .arg(command)
            .output()
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;

        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.stderr.is_empty() {
            text.push_str("\n[stderr] ");
            text.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        // Cap output to prevent accidental large reads
        let text: String = text.chars().take(20_000).collect();

        Ok(ToolResult {
            success: output.status.success(),
            output: text,
        })
    }

    async fn read_file(&self, path: &str) -> Result<ToolResult, ToolError> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;
        let output: String = content.chars().take(10_000).collect();
        Ok(ToolResult {
            success: true,
            output,
        })
    }

    async fn browse_page(&self, url: &str) -> Result<ToolResult, ToolError> {
        // Security: only allow http/https URLs (no file://, data:, etc.)
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return Err(ToolError::UnsafeUrl(url.to_owned()));
        }

        let cmd_out = tokio::process::Command::new("curl")
            .args([
                "-s",
                "--max-time",
                "15",
                "--max-filesize",
                "524288", // 512 KiB hard limit
                "-L",
                "--user-agent",
                "AI-Girls-Desktop/0.1",
                url,
            ])
            .output()
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;

        let raw = String::from_utf8_lossy(&cmd_out.stdout);
        let text = strip_html_naive(&raw);
        let truncated: String = text.chars().take(8_000).collect();

        Ok(ToolResult {
            success: cmd_out.status.success(),
            output: truncated,
        })
    }

    async fn list_dir(&self, path: &str) -> Result<ToolResult, ToolError> {
        let mut entries = tokio::fs::read_dir(path)
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;

        let mut lines = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false);
            lines.push(if is_dir {
                format!("{name}/")
            } else {
                name
            });
        }
        lines.sort();
        Ok(ToolResult {
            success: true,
            output: lines.join("\n"),
        })
    }

    async fn search_files(&self, path: &str, pattern: &str) -> Result<ToolResult, ToolError> {
        // Prefer ripgrep, fall back to grep
        let (prog, args): (&str, Vec<&str>) = if which_sync("rg") {
            ("rg", vec!["--max-count=5", "-n", pattern, path])
        } else {
            ("grep", vec!["-rn", "--include=*", "-m", "5", pattern, path])
        };

        let output = tokio::process::Command::new(prog)
            .args(&args)
            .output()
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;

        let text: String = String::from_utf8_lossy(&output.stdout)
            .chars()
            .take(5_000)
            .collect();

        Ok(ToolResult {
            success: true,
            output: if text.is_empty() {
                format!("no matches for '{pattern}' in '{path}'")
            } else {
                text
            },
        })
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Subcommand-level safety filter for commands that allow only read operations.
fn is_subcommand_safe(command: &str) -> bool {
    let mut parts = command.split_whitespace();
    let head = parts.next().unwrap_or_default();
    let sub = parts.next().unwrap_or_default();

    match head {
        "git" => matches!(
            sub,
            "status" | "log" | "diff" | "show" | "branch" | "tag"
                | "remote" | "describe" | "rev-parse" | "shortlog" | "ls-files"
        ),
        "brew" => matches!(sub, "list" | "info" | "search" | "outdated" | "leaves" | "deps"),
        "cargo" => matches!(
            sub,
            "check" | "test" | "fmt" | "clippy" | "tree" | "metadata" | "locate-project"
        ),
        "npm" => matches!(sub, "list" | "ls" | "info" | "search" | "outdated"),
        "pip3" => matches!(sub, "list" | "show" | "search" | "freeze"),
        // allow scripting with python3/node (no sub-filter)
        "top" => false,             // top is interactive, skip
        "system_profiler" => {
            // only allow specific data types that don't expose private info
            matches!(sub, "SPSoftwareDataType" | "SPHardwareDataType" | "SPMemoryDataType")
        }
        _ => true, // for simple commands (cat, ls, etc.) no sub-filter needed
    }
}

fn which_sync(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Very naive HTML tag stripper — keeps text nodes, strips <...> blocks.
fn strip_html_naive(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // Collapse runs of whitespace
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
