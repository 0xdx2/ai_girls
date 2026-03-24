// The elided lifetime `'_` in `tauri::State<'_, T>` parameters triggers a false-positive
// `used_underscore_binding` lint on return types. Allow it for this file.
#![allow(clippy::used_underscore_binding)]

mod ai_adapters;
mod avatar_runtime;
mod macos_integration;
mod orchestrator;
mod persona_system;
mod state_model;
mod tool_runtime;
mod voice_pipeline;

use std::{collections::VecDeque, sync::Arc};

use crate::ai_adapters::FallbackProvider;
use crate::macos_integration::MacOsIntegration;
use crate::orchestrator::{Orchestrator, RunSummary};
use crate::tool_runtime::ToolRuntime;
use crate::voice_pipeline::VoicePipeline;
use serde::Serialize;
use tauri::Manager;
use tokio::sync::Mutex;
use tracing::{error, info};

const MAX_EVENT_LOGS: usize = 200;

type AppOrchestrator = Orchestrator<FallbackProvider>;

#[derive(Clone)]
struct AppState {
    orchestrator: Arc<Mutex<AppOrchestrator>>,
    event_logs: Arc<Mutex<VecDeque<String>>>,
    /// Cached agent list (builtins + disk-scanned); populated once at startup.
    agents: Arc<Vec<serde_json::Value>>,
    /// Cached skill list (SKILL.md in skill-name subdirs); populated once at startup.
    skills: Arc<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunSummaryDto {
    answer: String,
    final_state: String,
    lipsync_frames: usize,
    audio_bytes: usize,
    /// Which AI provider (CLI) produced the answer.
    provider: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UiEvent {
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MacOsPreflight {
    is_macos: bool,
    automation_enabled: bool,
    accessibility: String,
    microphone: String,
    screen_recording: String,
    frontmost_app: Option<String>,
    frontmost_app_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StateChangeEvent {
    state: String,
    visual_hint: String,
    /// Fine-grained activity name (e.g. "`ThinkingDeep`", "Planning", "`UsingTool`").
    activity: String,
    /// CSS class for the activity overlay (e.g. "`activity_thinking_deep`").
    activity_hint: String,
}

impl From<RunSummary> for RunSummaryDto {
    fn from(value: RunSummary) -> Self {
        Self {
            answer: value.answer,
            final_state: format!("{:?}", value.final_state),
            lipsync_frames: value.lipsync_frames,
            audio_bytes: value.audio_bytes,
            provider: value.provider,
        }
    }
}

#[tauri::command]
#[allow(clippy::used_underscore_binding)]
async fn submit_text(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<RunSummaryDto, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("输入不能为空".to_owned());
    }

    let mut orchestrator = state.orchestrator.lock().await;
    let summary = orchestrator
        .handle_text_input(trimmed)
        .await
        .map_err(|err| err.to_string())?;

    Ok(summary.into())
}

#[tauri::command]
#[allow(clippy::used_underscore_binding)]
async fn submit_voice(
    input: String,
    state: tauri::State<'_, AppState>,
) -> Result<RunSummaryDto, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("语音文本不能为空".to_owned());
    }

    let mut orchestrator = state.orchestrator.lock().await;
    let summary = orchestrator
        .handle_voice_input(trimmed)
        .await
        .map_err(|err| err.to_string())?;

    Ok(summary.into())
}

#[tauri::command]
#[allow(clippy::used_underscore_binding)]
async fn recent_events(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    let logs = state.event_logs.lock().await;
    Ok(logs.iter().cloned().collect())
}

#[tauri::command]
#[allow(clippy::used_underscore_binding)]
async fn list_providers(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    let orchestrator = state.orchestrator.lock().await;
    Ok(orchestrator.available_providers())
}

#[tauri::command]
async fn health_check() -> &'static str {
    "ok"
}

#[tauri::command]
async fn macos_preflight() -> MacOsPreflight {
    let is_macos = MacOsIntegration::is_macos();
    let automation_enabled = std::env::var("ENABLE_MACOS_AUTOMATION").unwrap_or_default() == "1";
    let permissions = MacOsIntegration::new().check_permissions().await;
    let frontmost = if is_macos {
        MacOsIntegration::new().get_frontmost_app().await.map(Some)
    } else {
        Ok(None)
    };

    let (frontmost_app, frontmost_app_error) = match frontmost {
        Ok(value) => (value, None),
        Err(err) => (None, Some(err.to_string())),
    };

    MacOsPreflight {
        is_macos,
        automation_enabled,
        accessibility: format!("{:?}", permissions.accessibility),
        microphone: format!("{:?}", permissions.microphone),
        screen_recording: format!("{:?}", permissions.screen_recording),
        frontmost_app,
        frontmost_app_error,
    }
}

#[tauri::command]
fn detect_providers() -> serde_json::Value {
    use serde_json::json;
    // Use the same prefix rules as the actual provider constructors.
    let anthropic = std::env::var("ANTHROPIC_API_KEY")
        .map(|v| {
            let v = v.trim().to_owned();
            v.starts_with("sk-ant-")
        })
        .unwrap_or(false);

    let openai = std::env::var("OPENAI_API_KEY")
        .map(|v| {
            let v = v.trim().to_owned();
            v.starts_with("sk-") && !v.starts_with("sk-ant-")
        })
        .unwrap_or(false);

    let gemini = std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_API_KEY"))
        .map(|v| {
            let v = v.trim().to_owned();
            v.starts_with("AIza") && v.len() >= 20
        })
        .unwrap_or(false);

    let copilot_env = ["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"]
        .iter()
        .any(|k| {
            std::env::var(k)
                .map(|v| crate::ai_adapters::is_real_github_token(v.trim()))
                .unwrap_or(false)
        });

    let copilot_file = (|| -> Option<bool> {
        let home = std::env::var("HOME").ok()?;
        let apps = std::path::PathBuf::from(&home).join(".config/github-copilot/apps.json");
        if let Ok(content) = std::fs::read_to_string(apps) {
            if let Ok(serde_json::Value::Object(map)) =
                serde_json::from_str::<serde_json::Value>(&content)
            {
                if map.values().any(|v| {
                    v.get("oauth_token")
                        .and_then(|t| t.as_str())
                        .is_some_and(|t| crate::ai_adapters::is_real_github_token(t.trim()))
                }) {
                    return Some(true);
                }
            }
        }
        // hosts.json fallback
        let hosts = std::path::PathBuf::from(&home).join(".config/github-copilot/hosts.json");
        if let Ok(content) = std::fs::read_to_string(hosts) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                let ok = val
                    .get("github.com")
                    .and_then(|v| v.get("oauth_token"))
                    .and_then(|t| t.as_str())
                    .is_some_and(|t| crate::ai_adapters::is_real_github_token(t.trim()));
                if ok {
                    return Some(true);
                }
            }
        }
        Some(false)
    })()
    .unwrap_or(false);

    let claude_cli = std::process::Command::new("which")
        .arg("claude")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let codex_cli = std::process::Command::new("which")
        .arg("codex")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let gemini_cli = std::process::Command::new("which")
        .arg("gemini")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    json!({
        "anthropicKey": anthropic,
        "openaiKey":    openai,
        "copilotToken": copilot_env || copilot_file,
        "claudeCli":    claude_cli,
        "codexCli":     codex_cli,
        "geminiCli":    gemini_cli || gemini,
    })
}

// ─── helpers: parse YAML frontmatter from .md files ─────────────────────────
fn parse_frontmatter_field(content: &str, field: &str) -> Option<String> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let end = rest.find("---")?;
    let fm = &rest[..end];
    let prefix = format!("{field}:");
    for line in fm.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix(&prefix) {
            let val = val.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Some(val.to_owned());
            }
        }
    }
    None
}

/// Scan a directory for `.md` files (flat); return JSON entries with `id`, `name`,
/// `type:"custom"`, `source`, and optional extra frontmatter fields.
fn scan_md_dir(dir: &std::path::Path, extra_fields: &[&str]) -> Vec<serde_json::Value> {
    use serde_json::json;
    let source = dir
        .parent()
        .and_then(|p| p.file_name())
        .map_or_else(|| "custom".into(), |n| n.to_string_lossy().into_owned());

    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let name = parse_frontmatter_field(&content, "name").unwrap_or_else(|| stem.clone());
            let mut obj = serde_json::Map::new();
            obj.insert("id".into(), json!(format!("custom_{}", stem)));
            obj.insert("name".into(), json!(name));
            obj.insert("type".into(), json!("custom"));
            obj.insert("source".into(), json!(source.clone()));
            for f in extra_fields {
                let val = parse_frontmatter_field(&content, f).unwrap_or_default();
                obj.insert(f.to_string(), json!(val));
            }
            out.push(serde_json::Value::Object(obj));
        }
    }
    out
}

/// Scan a skills directory for immediate subdirectories containing `SKILL.md`.
/// Each subdir is a skill; metadata is parsed from its SKILL.md frontmatter.
fn scan_skill_dir(dir: &std::path::Path) -> Vec<serde_json::Value> {
    use serde_json::json;
    let source = dir
        .parent()
        .and_then(|p| p.file_name())
        .map_or_else(|| "custom".into(), |n| n.to_string_lossy().into_owned());

    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let subdir = entry.path();
        if !subdir.is_dir() {
            continue;
        }
        let skill_file = subdir.join("SKILL.md");
        if !skill_file.exists() {
            continue;
        }

        let stem = subdir
            .file_name()
            .map_or_else(|| "unknown".into(), |n| n.to_string_lossy().into_owned());
        let content = std::fs::read_to_string(&skill_file).unwrap_or_default();
        let name = parse_frontmatter_field(&content, "name").unwrap_or_else(|| stem.clone());
        let icon = parse_frontmatter_field(&content, "icon");
        let description = parse_frontmatter_field(&content, "description");

        let mut obj = serde_json::Map::new();
        obj.insert("id".into(), json!(format!("custom_{}", stem)));
        obj.insert("name".into(), json!(name));
        obj.insert("type".into(), json!("custom"));
        obj.insert("source".into(), json!(source.clone()));
        if let Some(v) = icon {
            obj.insert("icon".into(), json!(v));
        }
        if let Some(v) = description {
            obj.insert("description".into(), json!(v));
        }
        out.push(serde_json::Value::Object(obj));
    }
    out
}

/// Build the full agent list (builtins + disk scan) for caching in `AppState`.
fn build_agent_list() -> Vec<serde_json::Value> {
    use serde_json::json;
    let mut agents: Vec<serde_json::Value> = vec![
        json!({"id":"ask",   "name":"Ask",   "type":"builtin","icon":"💬","description":"单轮问答，直接回复"}),
        json!({"id":"plan",  "name":"Plan",  "type":"builtin","icon":"📋","description":"逐步推理，制定计划"}),
        json!({"id":"code",  "name":"Code",  "type":"builtin","icon":"💻","description":"代码为主，给出完整实现"}),
        json!({"id":"agent", "name":"Agent", "type":"builtin","icon":"🤖","description":"自主 Agent，调用工具完成任务"}),
    ];
    let home = std::env::var("HOME").unwrap_or_default();
    for dir_str in &[
        format!("{home}/.github/agents"),
        format!("{home}/.claude/agents"),
        format!("{home}/.copilot/agents"),
    ] {
        let dir = std::path::Path::new(dir_str);
        let mut entries = scan_md_dir(dir, &["icon", "description"]);
        agents.append(&mut entries);
    }
    agents
}

/// Build the full skill list (SKILL.md scan) for caching in `AppState`.
fn build_skill_list() -> Vec<serde_json::Value> {
    let mut skills: Vec<serde_json::Value> = vec![];
    let home = std::env::var("HOME").unwrap_or_default();
    for dir_str in &[
        format!("{home}/.github/skills"),
        format!("{home}/.claude/skills"),
        format!("{home}/.copilot/skills"),
        format!("{home}/.agents/skills"),
    ] {
        let dir = std::path::Path::new(dir_str);
        let mut entries = scan_skill_dir(dir);
        skills.append(&mut entries);
    }
    skills
}

#[tauri::command]
#[allow(clippy::used_underscore_binding)]
async fn list_models(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let orchestrator = state.orchestrator.lock().await;
    Ok(json!({ "models": orchestrator.list_models().await }))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn list_agents(state: tauri::State<'_, AppState>) -> serde_json::Value {
    use serde_json::json;
    json!({ "agents": *state.agents })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn list_skills(state: tauri::State<'_, AppState>) -> serde_json::Value {
    use serde_json::json;
    json!({ "skills": *state.skills })
}

#[tauri::command]
#[allow(clippy::used_underscore_binding)]
async fn set_active_model(
    provider: String,
    model: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // Map provider family → env var key used by the corresponding provider
    let env_key = match provider.as_str() {
        "anthropic" => "ANTHROPIC_MODEL",
        "google" => "GEMINI_MODEL",
        _ => "COPILOT_MODEL", // Copilot uses same key; OpenAI direct also respects it
    };
    std::env::set_var(env_key, &model);

    // Rebuild the provider stack so the new model takes effect immediately.
    let fresh = FallbackProvider::from_env();
    {
        let mut orchestrator = state.orchestrator.lock().await;
        let _ = orchestrator.pin_llm_provider(fresh);
    }
    info!("set_active_model: {provider} → {model} (env {env_key})");
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn open_system_prefs(pane: String) {
    let url = match pane.as_str() {
        "Accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        "Microphone" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        }
        "ScreenCapture" | "ScreenRecording" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
        }
        "Automation" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
        }
        _ => "x-apple.systempreferences:com.apple.preference.security?Privacy",
    };
    let _ = std::process::Command::new("open").arg(url).spawn();
}

#[tauri::command]
#[allow(clippy::used_underscore_binding)]
async fn save_api_key(
    key: String,
    value: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let env_path = std::path::PathBuf::from(".env");
    let existing = std::fs::read_to_string(&env_path).unwrap_or_default();
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| !l.starts_with(&format!("{key}=")))
        .map(std::borrow::ToOwned::to_owned)
        .collect();
    if !value.trim().is_empty() {
        lines.push(format!("{}={}", key, value.trim()));
    }
    std::fs::write(&env_path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    // Set for current process so FallbackProvider::from_env() sees it immediately.
    std::env::set_var(&key, value.trim());

    // Re-discover providers now that the env var is set, then hot-swap into
    // the running orchestrator so the new provider is available right away.
    let fresh = FallbackProvider::from_env();
    {
        let mut orchestrator = state.orchestrator.lock().await;
        let _ = orchestrator.pin_llm_provider(fresh);
    }
    info!("save_api_key: reloaded LLM providers after updating {key}");
    Ok(())
}

#[tauri::command]
#[allow(clippy::used_underscore_binding)]
async fn get_provider_quota(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let orchestrator = state.orchestrator.lock().await;
    Ok(orchestrator
        .check_quota()
        .await
        .unwrap_or(serde_json::Value::Null))
}

// ─── translate_agent_frontmatter helpers ─────────────────────────────────────

/// Frontmatter fields that should be translated (text-only; skip color, emoji, etc.)
const TRANSLATABLE_FIELDS: &[&str] = &["name", "description", "vibe"];

/// Returns true when `text` already contains at least one CJK character.
fn has_chinese(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{4e00}'..='\u{9fff}' |   // CJK Unified Ideographs
            '\u{3400}'..='\u{4dbf}' |   // Extension A
            '\u{f900}'..='\u{faff}'     // Compatibility Ideographs
        )
    })
}

/// Parse every key-value pair from YAML frontmatter between `---` delimiters.
fn parse_all_frontmatter_fields(content: &str) -> Vec<(String, String)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return vec![];
    }
    let rest = &content[3..];
    let Some(end) = rest.find("---") else {
        return vec![];
    };
    let mut pairs = Vec::new();
    for line in rest[..end].lines() {
        let line = line.trim();
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_owned();
            let val = line[colon + 1..]
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_owned();
            if !key.is_empty() && !val.is_empty() {
                pairs.push((key, val));
            }
        }
    }
    pairs
}

/// Rewrite the frontmatter of `content`, substituting values from `translations`.
/// Lines not present in the map are kept verbatim.
fn apply_frontmatter_translations(
    content: &str,
    translations: &std::collections::HashMap<String, String>,
) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content.to_owned();
    }
    let rest = &trimmed[3..];
    let Some(fm_end) = rest.find("---") else {
        return content.to_owned();
    };
    let fm_block = &rest[..fm_end];
    let body_after = &rest[fm_end + 3..]; // everything after the closing ---

    let mut new_fm = String::new();
    for line in fm_block.lines() {
        let trimmed_line = line.trim();
        let mut replaced = false;
        for (k, v) in translations {
            let prefix = format!("{k}:");
            if trimmed_line.starts_with(&prefix) {
                let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                new_fm.push_str(&format!("{indent}{k}: {v}\n"));
                replaced = true;
                break;
            }
        }
        if !replaced {
            new_fm.push_str(line);
            new_fm.push('\n');
        }
    }

    format!("---\n{}---{}", new_fm, body_after)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslateFileResult {
    path: String,
    /// "updated" | "skipped" | "error"
    status: String,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslateResult {
    total_files: usize,
    updated_files: usize,
    skipped_files: usize,
    errors: Vec<String>,
    details: Vec<TranslateFileResult>,
}

/// Scan all well-known agent directories, translate any un-translated frontmatter
/// fields in `.md` files to bilingual "English / 中文" format using the LLM.
#[tauri::command]
#[allow(clippy::used_underscore_binding)]
async fn translate_agent_frontmatter(
    state: tauri::State<'_, AppState>,
) -> Result<TranslateResult, String> {
    use std::collections::HashMap;

    let home = std::env::var("HOME").unwrap_or_default();
    let agent_dirs = [
        format!("{home}/.github/agents"),
        format!("{home}/.claude/agents"),
        format!("{home}/.copilot/agents"),
    ];

    // Collect all .md files across all agent dirs
    let mut md_files: Vec<std::path::PathBuf> = Vec::new();
    for dir_str in &agent_dirs {
        let dir = std::path::Path::new(dir_str);
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                md_files.push(path);
            }
        }
    }

    let mut result = TranslateResult {
        total_files: md_files.len(),
        updated_files: 0,
        skipped_files: 0,
        errors: vec![],
        details: vec![],
    };

    if md_files.is_empty() {
        return Ok(result);
    }

    for path in &md_files {
        let path_str = path.to_string_lossy().to_string();

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                let msg = e.to_string();
                result.errors.push(format!("{path_str}: {msg}"));
                result.details.push(TranslateFileResult {
                    path: path_str,
                    status: "error".into(),
                    message: Some(msg),
                });
                continue;
            }
        };

        // Find translatable fields that are not yet bilingual (no CJK characters)
        let to_translate: Vec<(String, String)> = parse_all_frontmatter_fields(&content)
            .into_iter()
            .filter(|(k, v)| TRANSLATABLE_FIELDS.contains(&k.as_str()) && !has_chinese(v))
            .collect();

        if to_translate.is_empty() {
            result.skipped_files += 1;
            result.details.push(TranslateFileResult {
                path: path_str,
                status: "skipped".into(),
                message: Some("already bilingual".into()),
            });
            continue;
        }

        // Build a single prompt to translate all fields at once
        let fields_text = to_translate
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Translate the following agent metadata fields from English to Chinese (Simplified). \
             Return ONLY a valid JSON object where keys are exactly the same as given and values \
             are the Chinese translations. Do NOT include any commentary, markdown fences, or \
             extra text. Example output: {{\"name\":\"代码审查员\",\"description\":\"提供专业反馈的审查员\"}}\n\n\
             Fields to translate:\n{fields_text}"
        );

        let mut orchestrator = state.orchestrator.lock().await;
        let response = orchestrator
            .handle_text_input(&prompt)
            .await
            .map_err(|err| err.to_string())?;

        // Extract JSON from response (tolerate leading/trailing prose)
        let raw = response.answer.trim();
        let json_str = if let (Some(s), Some(e)) = (raw.find('{'), raw.rfind('}')) {
            &raw[s..=e]
        } else {
            raw
        };

        let translations_json: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(e) => {
                let msg = format!("JSON parse error: {e} (raw: {json_str})");
                result.errors.push(format!("{path_str}: {msg}"));
                result.details.push(TranslateFileResult {
                    path: path_str,
                    status: "error".into(),
                    message: Some(msg),
                });
                continue;
            }
        };

        // Build bilingual values  "English original / 中文译文"
        let mut update_map: HashMap<String, String> = HashMap::new();
        for (k, en_val) in &to_translate {
            if let Some(zh_val) = translations_json.get(k).and_then(|v| v.as_str()) {
                let zh = zh_val.trim();
                if !zh.is_empty() {
                    update_map.insert(k.clone(), format!("{en_val} / {zh}"));
                }
            }
        }

        if update_map.is_empty() {
            result.skipped_files += 1;
            result.details.push(TranslateFileResult {
                path: path_str,
                status: "skipped".into(),
                message: Some("LLM returned no translations".into()),
            });
            continue;
        }

        let new_content = apply_frontmatter_translations(&content, &update_map);
        if let Err(e) = std::fs::write(path, new_content) {
            let msg = e.to_string();
            result.errors.push(format!("{path_str}: {msg}"));
            result.details.push(TranslateFileResult {
                path: path_str,
                status: "error".into(),
                message: Some(msg),
            });
            continue;
        }

        let updated_keys = update_map.keys().cloned().collect::<Vec<_>>().join(", ");
        result.updated_files += 1;
        result.details.push(TranslateFileResult {
            path: path_str,
            status: "updated".into(),
            message: Some(format!("translated fields: {updated_keys}")),
        });
    }

    Ok(result)
}

fn load_dotenv() {
    // 1. CWD .env (works for `cargo tauri dev` from src-tauri/)
    if dotenvy::dotenv().is_ok() {
        return;
    }
    // 2. Fallback locations: src-tauri/.env relative to known directories
    let candidates = [
        // Relative to workspace root when launched from project root
        std::path::PathBuf::from("apps/desktop-shell/src-tauri/.env"),
        // Parent of CWD (e.g. launched from apps/desktop-shell/)
        std::path::PathBuf::from("src-tauri/.env"),
    ];
    for path in &candidates {
        if path.exists() {
            let _ = dotenvy::from_path(path);
            return;
        }
    }
}

fn main() {
    load_dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let app_state = AppState {
        orchestrator: Arc::new(Mutex::new(Orchestrator::new(
            FallbackProvider::from_env(),
            ToolRuntime::default(),
            VoicePipeline::new(),
            MacOsIntegration::new(),
        ))),
        event_logs: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_EVENT_LOGS))),
        agents: Arc::new(build_agent_list()),
        skills: Arc::new(build_skill_list()),
    };

    tauri::Builder::default()
        .manage(app_state.clone())
        .invoke_handler(tauri::generate_handler![
            submit_text,
            submit_voice,
            recent_events,
            list_providers,
            health_check,
            macos_preflight,
            detect_providers,
            open_system_prefs,
            save_api_key,
            get_provider_quota,
            list_models,
            list_agents,
            list_skills,
            set_active_model,
            translate_agent_frontmatter
        ])
        .setup(move |app| {
            let app_handle = app.handle();
            let state: tauri::State<'_, AppState> = app.state();
            let state = state.inner().clone();

            tauri::async_runtime::spawn(async move {
                let mut receiver = {
                    let orchestrator = state.orchestrator.lock().await;
                    orchestrator.subscribe()
                };

                loop {
                    match receiver.recv().await {
                        Ok(event) => {
                            let message = format!("{event:?}");
                            {
                                let mut logs = state.event_logs.lock().await;
                                if logs.len() >= MAX_EVENT_LOGS {
                                    logs.pop_front();
                                }
                                logs.push_back(message.clone());
                            }

                            if let Err(err) = app_handle.emit_all(
                                "domain-event",
                                UiEvent {
                                    message: message.clone(),
                                },
                            ) {
                                error!("failed to emit domain-event to frontend: {err}");
                            }

                            if let crate::state_model::DomainEvent::AvatarStateChanged {
                                to,
                                activity,
                                ..
                            } = &event
                            {
                                let _ = app_handle.emit_all(
                                    "avatar-state",
                                    StateChangeEvent {
                                        state: format!("{to:?}"),
                                        visual_hint:
                                            crate::avatar_runtime::AvatarStateMachine::visual_hint(
                                                *to,
                                            )
                                            .to_owned(),
                                        activity: format!("{activity:?}"),
                                        activity_hint: activity.hint().to_owned(),
                                    },
                                );
                            }

                            info!("event: {message}");
                        }
                        Err(err) => {
                            error!("event stream closed: {err}");
                            break;
                        }
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run AI Girls Desktop Tauri app");
}
