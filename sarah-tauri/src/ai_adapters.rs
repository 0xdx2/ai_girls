use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

// ─── Parsed response ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ParsedResponse {
    pub thinking_blocks: Vec<String>,
    pub todos: Vec<(String, bool)>,
    pub code_blocks: Vec<(String, String)>,
    #[allow(dead_code)]
    pub clean_text: String,
}

pub fn parse_llm_output(text: &str) -> ParsedResponse {
    let mut thinking_blocks: Vec<String> = Vec::new();
    let mut code_blocks: Vec<(String, String)> = Vec::new();
    let mut todos: Vec<(String, bool)> = Vec::new();

    let mut after_thinking = String::new();
    let mut rest = text;
    while let Some(start_pos) = rest.find("<thinking>") {
        after_thinking.push_str(&rest[..start_pos]);
        rest = &rest[start_pos + "<thinking>".len()..];
        if let Some(end_pos) = rest.find("</thinking>") {
            let block = rest[..end_pos].trim().to_owned();
            if !block.is_empty() {
                thinking_blocks.push(block);
            }
            rest = &rest[end_pos + "</thinking>".len()..];
        } else {
            after_thinking.push_str("<thinking>");
            break;
        }
    }
    after_thinking.push_str(rest);

    let mut after_code = String::new();
    let mut rest2 = after_thinking.as_str();
    while let Some(fence_start) = rest2.find("```") {
        after_code.push_str(&rest2[..fence_start]);
        rest2 = &rest2[fence_start + 3..];
        let (lang, post_lang) = if let Some(nl) = rest2.find('\n') {
            (rest2[..nl].trim().to_owned(), &rest2[nl + 1..])
        } else {
            (String::new(), rest2)
        };
        if let Some(fence_end) = post_lang.find("```") {
            let preview: String = post_lang[..fence_end].chars().take(200).collect();
            let label = if lang.is_empty() { "text".to_owned() } else { lang };
            code_blocks.push((label, preview));
            rest2 = &post_lang[fence_end + 3..];
        } else {
            after_code.push_str("```");
            after_code.push_str(rest2);
            rest2 = "";
            break;
        }
    }
    after_code.push_str(rest2);

    for line in after_code.lines() {
        let t = line.trim();
        if let Some(item) = t.strip_prefix("- [ ] ") {
            todos.push((item.to_owned(), false));
        } else if let Some(item) = t.strip_prefix("- [x] ") {
            todos.push((item.to_owned(), true));
        } else if let Some(item) = t.strip_prefix("- [X] ") {
            todos.push((item.to_owned(), true));
        } else if let Some(item) = t.strip_prefix("TODO: ") {
            todos.push((item.to_owned(), false));
        }
    }

    ParsedResponse {
        thinking_blocks,
        todos,
        code_blocks,
        clean_text: after_code.trim().to_owned(),
    }
}

// ─── Response type ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub text: String,
    pub parsed: ParsedResponse,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub provider: &'static str,
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("no HTTP provider configured — set ANTHROPIC_API_KEY, OPENAI_API_KEY, GEMINI_API_KEY, or COPILOT_GITHUB_TOKEN")]
    NoProviderAvailable,
    #[error("HTTP request failed: {0}")]
    HttpFailed(String),
    #[allow(dead_code)]
    #[error("request timed out after {0:?}")]
    Timeout(Duration),
    #[error("empty response from provider")]
    EmptyResponse,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn complete(&self, prompt: &str) -> Result<LlmResponse, LlmError>;
    fn provider_summary(&self) -> Vec<&'static str> {
        vec![self.name()]
    }
    /// Key used when this provider's quota is included in the aggregate quota map.
    /// Override to expose a stable, user-friendly key (e.g. `"copilot"`).
    fn quota_key(&self) -> &'static str {
        self.name()
    }
    /// Check provider-specific usage / quota information.
    /// Returns `None` if unsupported or when credentials are unavailable.
    async fn check_quota(&self) -> Option<serde_json::Value> {
        None
    }
    /// Return the list of models this provider supports.
    /// Each entry is a JSON object: { id, name, provider, description? }.
    /// The default returns an empty list (providers that expose no model list).
    async fn list_models(&self) -> Vec<serde_json::Value> {
        vec![]
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// These prefixes indicate a real GitHub OAuth/PAT token, not a placeholder.
pub fn is_real_github_token(s: &str) -> bool {
    s.starts_with("ghu_")
        || s.starts_with("gho_")
        || s.starts_with("ghp_")
        || s.starts_with("github_pat_")
}

fn estimate_tokens(text: &str) -> u32 {
    (text.chars().count() as u32 / 4).max(1)
}

fn make_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("fatal: failed to build HTTP client")
}

#[doc(hidden)]
#[allow(dead_code)]
pub fn find_binary(name: &str) -> Option<std::path::PathBuf> {
    let _ = name;
    None
}

// ─── Claude HTTP Provider ─────────────────────────────────────────────────────

pub struct ClaudeHttpProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl ClaudeHttpProvider {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok()?.trim().to_owned();
        if !api_key.starts_with("sk-ant-") {
            return None;
        }
        let model = std::env::var("ANTHROPIC_MODEL")
            .unwrap_or_else(|_| "claude-opus-4-5".into());
        info!("ai_adapters: ClaudeHttpProvider active (model: {model})");
        Some(Self { client: make_client(), api_key, model })
    }
}

#[async_trait]
impl LlmProvider for ClaudeHttpProvider {
    fn name(&self) -> &'static str { "claude-http" }

    async fn list_models(&self) -> Vec<serde_json::Value> {
        use serde_json::json;
        vec![
            json!({"id":"claude-opus-4-5",       "name":"Claude Opus 4.5",       "provider":"anthropic"}),
            json!({"id":"claude-sonnet-4-5",     "name":"Claude Sonnet 4.5",     "provider":"anthropic"}),
            json!({"id":"claude-haiku-3-5",      "name":"Claude Haiku 3.5",      "provider":"anthropic"}),
            json!({"id":"claude-3-opus-20240229","name":"Claude 3 Opus",         "provider":"anthropic"}),
        ]
    }

    async fn complete(&self, prompt: &str) -> Result<LlmResponse, LlmError> {
        debug!("claude-http: POST /v1/messages model={}", self.model);
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": prompt}]
        });
        let resp = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::HttpFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::HttpFailed(format!("HTTP {status}: {body}")));
        }

        let json: serde_json::Value = resp.json().await
            .map_err(|e| LlmError::HttpFailed(e.to_string()))?;
        let text = json["content"][0]["text"]
            .as_str().ok_or(LlmError::EmptyResponse)?.to_owned();
        let prompt_tokens = json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
        let completion_tokens = json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;
        let parsed = parse_llm_output(&text);
        Ok(LlmResponse { text, parsed, prompt_tokens, completion_tokens, provider: "claude-http" })
    }
}

// ─── OpenAI HTTP Provider ─────────────────────────────────────────────────────

pub struct OpenAIHttpProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAIHttpProvider {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("OPENAI_API_KEY").ok()?.trim().to_owned();
        if !api_key.starts_with("sk-") {
            return None;
        }
        let model = std::env::var("OPENAI_MODEL")
            .unwrap_or_else(|_| "gpt-4o-mini".into());
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".into());
        info!("ai_adapters: OpenAIHttpProvider active (model: {model})");
        Some(Self { client: make_client(), api_key, model, base_url })
    }
}

#[async_trait]
impl LlmProvider for OpenAIHttpProvider {
    fn name(&self) -> &'static str { "openai-http" }

    async fn list_models(&self) -> Vec<serde_json::Value> {
        use serde_json::json;
        vec![
            json!({"id":"gpt-4.1",     "name":"GPT-4.1",          "provider":"openai"}),
            json!({"id":"gpt-4.1-mini","name":"GPT-4.1 Mini",     "provider":"openai"}),
            json!({"id":"gpt-4o",      "name":"GPT-4o",           "provider":"openai"}),
            json!({"id":"gpt-4o-mini", "name":"GPT-4o Mini",      "provider":"openai"}),
            json!({"id":"o3-mini",     "name":"o3 Mini",          "provider":"openai"}),
            json!({"id":"o1-mini",     "name":"o1 Mini",          "provider":"openai"}),
        ]
    }

    async fn complete(&self, prompt: &str) -> Result<LlmResponse, LlmError> {
        debug!("openai-http: POST /v1/chat/completions model={}", self.model);
        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}]
        });
        let resp = self.client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::HttpFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::HttpFailed(format!("HTTP {status}: {body}")));
        }

        let json: serde_json::Value = resp.json().await
            .map_err(|e| LlmError::HttpFailed(e.to_string()))?;
        let text = json["choices"][0]["message"]["content"]
            .as_str().ok_or(LlmError::EmptyResponse)?.to_owned();
        let prompt_tokens = json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32;
        let completion_tokens = json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;
        let parsed = parse_llm_output(&text);
        Ok(LlmResponse { text, parsed, prompt_tokens, completion_tokens, provider: "openai-http" })
    }
}

// ─── Copilot HTTP Provider ────────────────────────────────────────────────────

pub struct CopilotHttpProvider {
    client: reqwest::Client,
    /// Candidate GitHub OAuth tokens ordered by preference.  At least one must be
    /// present for the provider to be active.  Token exchange is attempted in order;
    /// the first success wins.
    github_tokens: Vec<String>,
    model: String,
    /// The most recently successful GitHub token for the quota endpoint.
    /// Avoids iterating on every call — only falls back to full iteration
    /// when the cached token is absent or returns a non-2xx response.
    cached_quota_token: std::sync::RwLock<Option<String>>,
}

impl CopilotHttpProvider {
    pub fn from_env() -> Option<Self> {
        let tokens = Self::collect_all_github_tokens();
        if tokens.is_empty() {
            return None;
        }
        let model = std::env::var("COPILOT_MODEL")
            .unwrap_or_else(|_| "gpt-4o".into());
        info!("ai_adapters: CopilotHttpProvider active ({} token candidate(s), model: {model})", tokens.len());
        Some(Self {
            client: make_client(),
            github_tokens: tokens,
            model,
            cached_quota_token: std::sync::RwLock::new(None),
        })
    }

    /// Collect every real GitHub token from all known sources, deduped, ordered
    /// by preference: env-var → hosts.json → apps.json (VSCode/Neovim first,
    /// then all remaining entries).
    fn collect_all_github_tokens() -> Vec<String> {
        let mut tokens: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut push = |t: String| {
            if seen.insert(t.clone()) { tokens.push(t); }
        };

        // 1. Explicit env vars
        for var in &["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"] {
            if let Ok(v) = std::env::var(var) {
                let v = v.trim().to_owned();
                if is_real_github_token(&v) {
                    info!("ai_adapters: GitHub token from env var {var} ({}...)", &v[..v.len().min(8)]);
                    push(v);
                } else if !v.is_empty() {
                    warn!("ai_adapters: env var {var} present but not a real GitHub token (placeholder?)");
                }
            }
        }

        // 2. hosts.json
        if let Some(t) = Self::token_from_hosts_json() { push(t); }

        // 3. apps.json — priority keys first, then remaining
        let (priority, rest) = Self::tokens_from_apps_json();
        for t in priority { push(t); }
        for t in rest     { push(t); }

        tokens
    }

    /// Reads from ~/.config/github-copilot/hosts.json
    /// Format: {"github.com": {"oauth_token": "ghu_..."}}
    fn token_from_hosts_json() -> Option<String> {
        let home = std::env::var("HOME").ok()?;
        let path = std::path::PathBuf::from(home)
            .join(".config/github-copilot/hosts.json");
        let content = std::fs::read_to_string(&path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        let token = json.get("github.com")
            .and_then(|v| v.get("oauth_token"))
            .and_then(|t| t.as_str())
            .map(|t| t.trim().to_owned())
            .filter(|t| is_real_github_token(t))?;
        info!("ai_adapters: GitHub token from hosts.json ({}...)", &token[..token.len().min(8)]);
        Some(token)
    }

    /// Returns (priority_tokens, other_tokens) from apps.json.
    /// Priority: keys containing VS Code client ID or Neovim client ID.
    fn tokens_from_apps_json() -> (Vec<String>, Vec<String>) {
        let empty = (Vec::new(), Vec::new());
        let home = match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => return empty,
        };
        let path = std::path::PathBuf::from(home)
            .join(".config/github-copilot/apps.json");
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return empty,
        };
        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => return empty,
        };
        let map = match json.as_object() {
            Some(m) => m,
            None => return empty,
        };
        let mut priority: Vec<String> = Vec::new();
        let mut rest: Vec<String> = Vec::new();
        for (key, val) in map {
            if let Some(tok) = val.get("oauth_token")
                .and_then(|t| t.as_str())
                .map(|t| t.trim().to_owned())
                .filter(|t| is_real_github_token(t))
            {
                if key.contains("Iv1.b507a08c87ecfe98") || key.contains("Iv23c") {
                    info!("ai_adapters: GitHub token from apps.json priority key ({}...)", &tok[..tok.len().min(8)]);
                    priority.push(tok);
                } else {
                    info!("ai_adapters: GitHub token from apps.json other key ({}...)", &tok[..tok.len().min(8)]);
                    rest.push(tok);
                }
            }
        }
        (priority, rest)
    }

    /// Try to exchange one GitHub OAuth token for a short-lived Copilot API token.
    async fn try_exchange_token(&self, github_token: &str) -> Result<String, LlmError> {
        let prefix = &github_token[..github_token.len().min(12)];
        debug!("copilot-http: trying token exchange for {prefix}...");
        let resp = self.client
            .get("https://api.github.com/copilot_internal/v2/token")
            .header("Authorization", format!("token {github_token}"))
            .header("Accept", "application/json")
            .header("X-Github-Api-Version", "2025-04-01")
            .header("Editor-Version", "vscode/1.96.2")
            .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
            .header("User-Agent", "GitHubCopilotChat/0.26.7")
            .send()
            .await
            .map_err(|e| LlmError::HttpFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::HttpFailed(
                format!("HTTP {status}: {body}")
            ));
        }

        let json: serde_json::Value = resp.json().await
            .map_err(|e| LlmError::HttpFailed(e.to_string()))?;
        json["token"]
            .as_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| LlmError::HttpFailed("no 'token' field in copilot_internal response".into()))
    }

    /// Attempt token exchange with each candidate GitHub OAuth token in order.
    /// Returns the first short-lived Copilot API token that succeeds.
    async fn get_copilot_token(&self) -> Result<String, LlmError> {
        let mut last_err = LlmError::HttpFailed("no GitHub tokens available".into());
        for github_token in &self.github_tokens {
            match self.try_exchange_token(github_token).await {
                Ok(api_token) => {
                    debug!("copilot-http: token exchange succeeded for {}...", &github_token[..github_token.len().min(12)]);
                    return Ok(api_token);
                }
                Err(e) => {
                    warn!("copilot-http: token {}... failed exchange — {e}", &github_token[..github_token.len().min(12)]);
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }
}

#[async_trait]
impl LlmProvider for CopilotHttpProvider {
    fn name(&self) -> &'static str { "copilot-http" }

    fn quota_key(&self) -> &'static str { "copilot" }

    async fn list_models(&self) -> Vec<serde_json::Value> {
        let fallback = vec![];

        // Exchange a GitHub token for a short-lived Copilot API token.
        let api_token = match self.get_copilot_token().await {
            Ok(t) => t,
            Err(e) => {
                warn!("copilot-http: list_models — token exchange failed ({e}), using static list");
                return fallback;
            }
        };

        let resp = match self.client
            .get("https://api.githubcopilot.com/models")
            .header("Authorization", format!("Bearer {api_token}"))
            .header("Editor-Version", "vscode/1.96.2")
            .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
            .header("User-Agent", "GitHubCopilotChat/0.26.7")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!("copilot-http: list_models — HTTP error ({e}), using static list");
                return fallback;
            }
        };

        if !resp.status().is_success() {
            warn!("copilot-http: list_models — HTTP {} , using static list", resp.status());
            return fallback;
        }

        let json: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                warn!("copilot-http: list_models — parse error ({e}), using static list");
                return fallback;
            }
        };

        // Response shape: { "data": [ { "id", "name", "vendor", "version", ... } ] }
        let models: Vec<serde_json::Value> = json
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        let id = m.get("id")?.as_str()?;
                        // Prefer the human-readable "name" field; fall back to id.
                        let name = m.get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or(id);
                        let vendor = m.get("vendor")
                            .and_then(|v| v.as_str())
                            .unwrap_or("copilot");
                        Some(serde_json::json!({
                            "id":       id,
                            "name":     name,
                            "provider": "copilot",
                            "vendor":   vendor,
                        }))
                    })
                    .collect()
            })
            .unwrap_or_default();

        if models.is_empty() {
            warn!("copilot-http: list_models — empty data array, using static list");
            fallback
        } else {
            debug!("copilot-http: list_models — {} model(s) from API", models.len());
            models
        }
    }

    async fn check_quota(&self) -> Option<serde_json::Value> {
        if self.github_tokens.is_empty() {
            return None;
        }

        // Read the previously successful token (if any) to try it first,
        // short-circuiting the full iteration on the happy path.
        let cached: Option<String> = self.cached_quota_token
            .read()
            .ok()
            .and_then(|g| g.clone());

        // Build ordered candidate list: cached token first (deduped), then rest.
        let mut ordered: Vec<&str> = Vec::new();
        if let Some(ref c) = cached {
            ordered.push(c.as_str());
        }
        for t in &self.github_tokens {
            if cached.as_deref() != Some(t.as_str()) {
                ordered.push(t.as_str());
            }
        }

        let mut json: Option<serde_json::Value> = None;
        for token in &ordered {
            let short = &token[..token.len().min(12)];
            let resp = match self.client
                .get("https://api.github.com/copilot_internal/user")
                .header("Authorization", format!("token {token}"))
                .header("Accept", "application/json")
                .header("X-Github-Api-Version", "2025-04-01")
                .header("Editor-Version", "vscode/1.96.2")
                .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
                .header("User-Agent", "GitHubCopilotChat/0.26.7")
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!("copilot-http: quota token {short}... request error — {e}");
                    continue;
                }
            };
            if !resp.status().is_success() {
                warn!("copilot-http: quota token {short}... HTTP {} — trying next", resp.status());
                // If this was the cached token, wipe it so the next call re-iterates.
                if cached.as_deref() == Some(*token) {
                    if let Ok(mut g) = self.cached_quota_token.write() {
                        *g = None;
                    }
                }
                continue;
            }
            // Cache the winner if it differs from what was stored.
            if cached.as_deref() != Some(*token) {
                if let Ok(mut g) = self.cached_quota_token.write() {
                    *g = Some(token.to_string());
                }
                debug!("copilot-http: quota — cached token {short}...");
            }
            json = resp.json().await.ok();
            debug!("copilot-http: quota token {short}... succeeded");
            break;
        }
        let json = json?;
        // The API returns snake_case fields (confirmed via vscode-copilot-chat source):
        //   quota_snapshots.premium_interactions.percent_remaining
        //   quota_snapshots.chat.percent_remaining
        let premium_pct = json
            .pointer("/quota_snapshots/premium_interactions/percent_remaining")
            .and_then(|v| v.as_f64());
        let premium_remaining = json
            .pointer("/quota_snapshots/premium_interactions/remaining")
            .and_then(|v| v.as_i64());
        let premium_entitlement = json
            .pointer("/quota_snapshots/premium_interactions/entitlement")
            .and_then(|v| v.as_i64());
        let premium_unlimited = json
            .pointer("/quota_snapshots/premium_interactions/unlimited")
            .and_then(|v| v.as_bool());
        let chat_pct = json
            .pointer("/quota_snapshots/chat/percent_remaining")
            .and_then(|v| v.as_f64());
        let plan = json.get("copilot_plan")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);

        debug!(
            "copilot-http: quota — premium={:?}% ({:?}/{:?}) unlimited={:?} chat={:?}% plan={:?}",
            premium_pct, premium_remaining, premium_entitlement, premium_unlimited, chat_pct, plan
        );
        serde_json::to_value(CopilotQuota {
            premium_percent_remaining: premium_pct,
            premium_remaining,
            premium_entitlement,
            premium_unlimited,
            chat_percent_remaining: chat_pct,
            plan,
        }).ok()
    }

    async fn complete(&self, prompt: &str) -> Result<LlmResponse, LlmError> {
        debug!("copilot-http: obtaining API token...");
        let api_token = self.get_copilot_token().await?;

        debug!("copilot-http: POST chat/completions model={}", self.model);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}]
        });
        let resp = self.client
            .post("https://api.githubcopilot.com/chat/completions")
            .header("Authorization", format!("Bearer {api_token}"))
            .header("Editor-Version", "vscode/1.96.2")
            .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
            .header("User-Agent", "GitHubCopilotChat/0.26.7")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::HttpFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::HttpFailed(format!("HTTP {status}: {body}")));
        }

        let json: serde_json::Value = resp.json().await
            .map_err(|e| LlmError::HttpFailed(e.to_string()))?;
        let text = json["choices"][0]["message"]["content"]
            .as_str().ok_or(LlmError::EmptyResponse)?.to_owned();
        let prompt_tokens = json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32;
        let completion_tokens = json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;
        let parsed = parse_llm_output(&text);
        Ok(LlmResponse { text, parsed, prompt_tokens, completion_tokens, provider: "copilot-http" })
    }
}

// ─── Copilot Quota ────────────────────────────────────────────────────────────

/// Copilot-specific quota snapshot returned under the `"copilot"` key.
/// Fields mirror the `/copilot_internal/user` API response (snake_case).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CopilotQuota {
    /// 0–100 percent of premium interactions (Claude/GPT-4o) remaining
    pub premium_percent_remaining: Option<f64>,
    /// Raw remaining count of premium requests
    pub premium_remaining: Option<i64>,
    /// Total allotted premium requests this period
    pub premium_entitlement: Option<i64>,
    /// Whether premium requests are unlimited (e.g. Pro plan)
    pub premium_unlimited: Option<bool>,
    /// 0–100 percent of base chat quota remaining
    pub chat_percent_remaining: Option<f64>,
    /// Plan label e.g. "individual", "individual_pro", "business"
    pub plan: Option<String>,
}

// ─── Gemini HTTP Provider ─────────────────────────────────────────────────────

pub struct GeminiHttpProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl GeminiHttpProvider {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .ok()?
            .trim()
            .to_owned();
        // Gemini keys are 39-char alphanumeric strings starting with "AIza"
        if !api_key.starts_with("AIza") || api_key.len() < 20 {
            return None;
        }
        let model = std::env::var("GEMINI_MODEL")
            .unwrap_or_else(|_| "gemini-2.0-flash".into());
        info!("ai_adapters: GeminiHttpProvider active (model: {model})");
        Some(Self { client: make_client(), api_key, model })
    }
}

#[async_trait]
impl LlmProvider for GeminiHttpProvider {
    fn name(&self) -> &'static str { "gemini-http" }

    async fn list_models(&self) -> Vec<serde_json::Value> {
        use serde_json::json;
        vec![
            json!({"id":"gemini-2.0-flash",      "name":"Gemini 2.0 Flash",      "provider":"google"}),
            json!({"id":"gemini-2.0-flash-lite", "name":"Gemini 2.0 Flash Lite",  "provider":"google"}),
            json!({"id":"gemini-1.5-pro",        "name":"Gemini 1.5 Pro",         "provider":"google"}),
            json!({"id":"gemini-1.5-flash",      "name":"Gemini 1.5 Flash",       "provider":"google"}),
        ]
    }

    async fn complete(&self, prompt: &str) -> Result<LlmResponse, LlmError> {
        debug!("gemini-http: POST generateContent model={}", self.model);
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );
        let body = serde_json::json!({
            "contents": [{"parts": [{"text": prompt}]}]
        });
        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::HttpFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::HttpFailed(format!("HTTP {status}: {body}")));
        }

        let json: serde_json::Value = resp.json().await
            .map_err(|e| LlmError::HttpFailed(e.to_string()))?;
        let text = json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str().ok_or(LlmError::EmptyResponse)?.to_owned();
        let prompt_tokens =
            json["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0) as u32;
        let completion_tokens =
            json["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0) as u32;
        let parsed = parse_llm_output(&text);
        Ok(LlmResponse { text, parsed, prompt_tokens, completion_tokens, provider: "gemini-http" })
    }
}

// ─── Mock Provider ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct MockProvider;

#[async_trait]
impl LlmProvider for MockProvider {
    fn name(&self) -> &'static str { "mock" }

    async fn complete(&self, prompt: &str) -> Result<LlmResponse, LlmError> {
        let preview: String = prompt.chars().take(120).collect();
        let text = format!(
            "[AI Girls 内置回复]\n\n已收到: {preview}\n\n\
             （请在 .env 设置 API Key）\n\
             • ANTHROPIC_API_KEY — Claude\n\
             • OPENAI_API_KEY    — GPT / 兼容 API\n\
             • GEMINI_API_KEY    — Gemini\n\
             • COPILOT_GITHUB_TOKEN — GitHub Copilot"
        );
        Ok(LlmResponse {
            text,
            parsed: ParsedResponse::default(),
            prompt_tokens: estimate_tokens(prompt),
            completion_tokens: 40,
            provider: "mock",
        })
    }
}

// ─── FallbackProvider ────────────────────────────────────────────────────────

pub struct FallbackProvider {
    providers: Vec<Box<dyn LlmProvider>>,
}

impl FallbackProvider {
    pub fn from_env() -> Self {
        let mut providers: Vec<Box<dyn LlmProvider>> = Vec::new();

        if let Some(p) = ClaudeHttpProvider::from_env() {
            providers.push(Box::new(p));
        }
        if let Some(p) = CopilotHttpProvider::from_env() {
            providers.push(Box::new(p));
        }
        if let Some(p) = OpenAIHttpProvider::from_env() {
            providers.push(Box::new(p));
        }
        if let Some(p) = GeminiHttpProvider::from_env() {
            providers.push(Box::new(p));
        }

        if providers.is_empty() {
            warn!(
                "ai_adapters: no HTTP provider configured — set one of:\n\
                 ANTHROPIC_API_KEY, OPENAI_API_KEY, GEMINI_API_KEY / GOOGLE_API_KEY,\n\
                 COPILOT_GITHUB_TOKEN / GH_TOKEN"
            );
        } else {
            let names: Vec<_> = providers.iter().map(|p| p.name()).collect();
            info!("ai_adapters: {} provider(s) active: {names:?}", names.len());
        }

        providers.push(Box::new(MockProvider));
        Self { providers }
    }
}

#[async_trait]
impl LlmProvider for FallbackProvider {
    fn name(&self) -> &'static str { "fallback" }

    async fn list_models(&self) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        for p in &self.providers {
            if p.name() != "mock" {
                out.extend(p.list_models().await);
            }
        }
        out
    }

    fn provider_summary(&self) -> Vec<&'static str> {
        self.providers
            .iter()
            .filter(|p| p.name() != "mock" && p.name() != "fallback")
            .map(|p| p.name())
            .collect()
    }

    /// Collects quota from every non-mock sub-provider and returns them as a
    /// JSON object keyed by each provider's `quota_key()`.  Returns `None`
    /// when no provider reports any quota data.
    async fn check_quota(&self) -> Option<serde_json::Value> {
        let mut map = serde_json::Map::new();
        for p in &self.providers {
            if p.name() == "mock" { continue; }
            if let Some(q) = p.check_quota().await {
                map.insert(p.quota_key().to_owned(), q);
            }
        }
        if map.is_empty() { None } else { Some(serde_json::Value::Object(map)) }
    }

    async fn complete(&self, prompt: &str) -> Result<LlmResponse, LlmError> {
        for provider in &self.providers {
            match provider.complete(prompt).await {
                Ok(resp) => {
                    info!("ai_adapters: got response from {}", resp.provider);
                    return Ok(resp);
                }
                Err(e) => {
                    warn!("ai_adapters: {} failed — {e}", provider.name());
                }
            }
        }
        Err(LlmError::NoProviderAvailable)
    }
}
