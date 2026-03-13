use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::ai_adapters::{LlmError, LlmProvider, LlmResponse, ParsedResponse};
use crate::avatar_runtime::AvatarStateMachine;
use crate::macos_integration::{MacOsError, MacOsIntegration};
use crate::persona_system::PersonaManager;
use crate::state_model::{
    AgentTask, AvatarState, DomainEvent, PermissionType, TokenUsageMetric, ToolCategory,
};
use thiserror::Error;
use crate::tool_runtime::{ToolError, ToolRequest, ToolRuntime};
use crate::voice_pipeline::VoicePipeline;

#[derive(Debug)]
pub struct RunSummary {
    pub answer: String,
    pub final_state: AvatarState,
    pub lipsync_frames: usize,
    pub audio_bytes: usize,
    /// Which AI provider produced the answer.
    pub provider: String,
}

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("llm error: {0}")]
    Llm(#[from] LlmError),
    #[error("tool error: {0}")]
    Tool(#[from] ToolError),
    #[error("macos integration error: {0}")]
    MacOs(#[from] MacOsError),
}

pub struct Orchestrator<P: LlmProvider> {
    fsm: AvatarStateMachine,
    llm: P,
    tools: ToolRuntime,
    voice: VoicePipeline,
    macos: MacOsIntegration,
    tx: tokio::sync::broadcast::Sender<DomainEvent>,
    task_counter: AtomicU64,
    persona: PersonaManager,
}

impl<P: LlmProvider> Orchestrator<P> {
    /// Returns the names of all discovered AI providers.
    pub fn available_providers(&self) -> Vec<String> {
        self.llm
            .provider_summary()
            .into_iter()
            .map(String::from)
            .collect()
    }

    pub fn new(
        llm: P,
        tools: ToolRuntime,
        voice: VoicePipeline,
        macos: MacOsIntegration,
    ) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            fsm: AvatarStateMachine::default(),
            llm,
            tools,
            voice,
            macos,
            tx,
            task_counter: AtomicU64::new(1),
            persona: PersonaManager::new(),
        }
    }

    pub fn pin_llm_provider(&mut self, p: P) -> Result<(), OrchestratorError> {
        self.llm = p;
        Ok(())
    }

    /// Delegates to the underlying LLM provider's quota check.
    pub async fn check_quota(&self) -> Option<serde_json::Value> {
        self.llm.check_quota().await
    }

    /// Returns the model list from the underlying LLM provider.
    pub async fn list_models(&self) -> Vec<serde_json::Value> {
        self.llm.list_models().await
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DomainEvent> {
        self.tx.subscribe()
    }

    pub async fn handle_text_input(&mut self, input: &str) -> Result<RunSummary, OrchestratorError> {
        self.emit_persona_changed(input);
        self.emit_and_reduce(DomainEvent::UserInputReceived {
            text: input.to_owned(),
        });

        let task = AgentTask {
            id: self.task_counter.fetch_add(1, Ordering::Relaxed),
            prompt: input.to_owned(),
            created_at_ms: now_ms(),
        };

        if let Some(command) = input.strip_prefix("/tool ") {
            return self.handle_tool_command(&task, command).await;
        }

        if let Some(path) = input.strip_prefix("/read ") {
            return self.handle_read_command(&task, path).await;
        }

        if let Some(raw) = input.strip_prefix("/act ") {
            return self.handle_action_command(&task, raw).await;
        }

        self.emit_and_reduce(DomainEvent::AgentThinkingStarted { task_id: task.id });
        let response = self.llm.complete(&task.prompt).await?;
        self.emit_and_reduce(DomainEvent::AgentThinkingFinished { task_id: task.id });

        // Emit rich agent-internal events from the parsed response so the avatar
        // can reflect thinking chunks, plans, code generation, etc.
        self.emit_agent_events(task.id, &response.parsed);

        self.finalize_answer(task.id, response).await
    }

    pub async fn handle_voice_input(
        &mut self,
        spoken_text: &str,
    ) -> Result<RunSummary, OrchestratorError> {
        let transcript = self.voice.transcribe_mock(spoken_text).await;
        self.emit_and_reduce(DomainEvent::AsrFinal {
            text: transcript.clone(),
        });
        self.handle_text_input(&transcript).await
    }

    async fn handle_tool_command(
        &mut self,
        task: &AgentTask,
        command: &str,
    ) -> Result<RunSummary, OrchestratorError> {
        self.emit_and_reduce(DomainEvent::ToolCallStarted {
            tool: "terminal".to_owned(),
            action: command.to_owned(),
            category: ToolCategory::Terminal,
        });

        let result = self
            .tools
            .invoke(ToolRequest::Terminal {
                command: command.to_owned(),
            })
            .await?;

        self.emit_and_reduce(DomainEvent::ToolCallFinished {
            tool: "terminal".to_owned(),
            success: result.success,
            output: result.output.clone(),
        });

        self.finalize_answer(
            task.id,
            LlmResponse {
                text: result.output,
                parsed: ParsedResponse::default(),
                prompt_tokens: 16,
                completion_tokens: 32,
                provider: "tool",
            },
        )
        .await
    }

    async fn handle_read_command(
        &mut self,
        task: &AgentTask,
        path: &str,
    ) -> Result<RunSummary, OrchestratorError> {
        self.emit_and_reduce(DomainEvent::ToolCallStarted {
            tool: "filesystem".to_owned(),
            action: path.to_owned(),
            category: ToolCategory::Filesystem,
        });

        let result = self
            .tools
            .invoke(ToolRequest::ReadFile {
                path: path.to_owned(),
            })
            .await?;

        self.emit_and_reduce(DomainEvent::ToolCallFinished {
            tool: "filesystem".to_owned(),
            success: result.success,
            output: format!("read {} chars", result.output.chars().count()),
        });

        self.finalize_answer(
            task.id,
            LlmResponse {
                text: result.output,
                parsed: ParsedResponse::default(),
                prompt_tokens: 24,
                completion_tokens: 24,
                provider: "filesystem",
            },
        )
        .await
    }

    async fn handle_action_command(
        &mut self,
        task: &AgentTask,
        raw: &str,
    ) -> Result<RunSummary, OrchestratorError> {
        let permissions = self.macos.check_permissions().await;
        if permissions.accessibility != crate::state_model::PermissionStatus::Granted {
            self.emit_and_reduce(DomainEvent::PermissionRequired {
                permission: PermissionType::Accessibility,
            });
        }

        let (target, text) = parse_action(raw);
        self.emit_and_reduce(DomainEvent::SystemActionRequested {
            action: "safe_input".to_owned(),
            target: target.to_owned(),
        });

        let result = self.macos.perform_safe_input(target, text).await;
        let success = result.is_ok();

        self.emit_and_reduce(DomainEvent::SystemActionExecuted {
            action: "safe_input".to_owned(),
            target: target.to_owned(),
            success,
        });

        let answer = match result {
            Ok(msg) => msg,
            Err(err) => format!("action failed: {err}"),
        };

        self.finalize_answer(
            task.id,
            LlmResponse {
                text: answer,
                parsed: ParsedResponse::default(),
                prompt_tokens: 10,
                completion_tokens: 12,
                provider: "macos-action",
            },
        )
        .await
    }

    async fn finalize_answer(
        &mut self,
        task_id: u64,
        response: LlmResponse,
    ) -> Result<RunSummary, OrchestratorError> {
        self.emit_and_reduce(DomainEvent::AgentAnswerReady {
            task_id,
            answer: response.text.clone(),
        });

        let total_tokens = response.prompt_tokens.saturating_add(response.completion_tokens);
        self.emit_and_reduce(DomainEvent::TokenUsageUpdated {
            metric: TokenUsageMetric {
                prompt_tokens: response.prompt_tokens,
                completion_tokens: response.completion_tokens,
                total_tokens,
            },
        });

        self.emit_and_reduce(DomainEvent::VoicePlaybackStarted);
        let audio = self.voice.synthesize(&response.text).await;
        let lipsync = self
            .voice
            .lipsync_frames(&response.text, self.voice.estimate_duration_ms(&response.text));
        self.emit_and_reduce(DomainEvent::LipSyncFramesGenerated {
            frame_count: lipsync.len(),
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        self.emit_and_reduce(DomainEvent::VoicePlaybackFinished);
        self.emit_and_reduce(DomainEvent::TaskCompleted { task_id });

        Ok(RunSummary {
            answer: response.text,
            final_state: self.fsm.current(),
            lipsync_frames: lipsync.len(),
            audio_bytes: audio.len(),
            provider: response.provider.to_owned(),
        })
    }

    fn emit_and_reduce(&mut self, event: DomainEvent) {
        let prev = self.fsm.current();
        let next = self.fsm.reduce(&event);
        let _ = self.tx.send(event);

        if prev != next {
            let activity = self.fsm.current_activity();
            let _ = self.tx.send(DomainEvent::AvatarStateChanged {
                from: prev,
                to: next,
                activity,
            });
        }
    }

    /// Emit fine-grained agent-internal events derived from the parsed LLM response.
    /// These are the same categories VS Code's Agent Debug Panel surfaces — but
    /// instead of a debug view they drive avatar state changes and animations.
    fn emit_agent_events(&mut self, task_id: u64, parsed: &ParsedResponse) {
        // Each reasoning chunk escalates the avatar toward ThinkingDeep
        for chunk in &parsed.thinking_blocks {
            self.emit_and_reduce(DomainEvent::ModelThinkingChunk {
                task_id,
                text: chunk.clone(),
            });
        }

        // If the model produced a structured task plan, avatar enters Planning
        if !parsed.todos.is_empty() {
            let todos: Vec<String> = parsed.todos.iter().map(|(t, _)| t.clone()).collect();
            self.emit_and_reduce(DomainEvent::AgentPlanCreated { task_id, todos });
            for (i, (title, done)) in parsed.todos.iter().enumerate() {
                self.emit_and_reduce(DomainEvent::AgentTodoUpdated {
                    task_id,
                    index: i,
                    title: title.clone(),
                    done: *done,
                });
            }
        }

        // Each code block puts the avatar into GeneratingCode pose
        for (language, preview) in &parsed.code_blocks {
            self.emit_and_reduce(DomainEvent::AgentCodeGenerated {
                task_id,
                language: language.clone(),
                preview: preview.clone(),
            });
        }
    }

    fn emit_persona_changed(&mut self, context: &str) {
        let profile = self.persona.infer_and_switch(context).clone();
        let _ = self.tx.send(DomainEvent::PersonaChanged {
            role: profile.role.label().to_owned(),
            display_name: profile.display_name,
            icon: profile.icon.to_owned(),
            accent_color: profile.accent_color.to_owned(),
            glow_color: profile.glow_color.to_owned(),
            costume_tag: profile.costume_tag.to_owned(),
            live2d_expression: profile.live2d_expression.to_owned(),
            live2d_motion: profile.live2d_motion.to_owned(),
        });
    }
}

fn parse_action(raw: &str) -> (&str, &str) {
    if let Some((app, text)) = raw.split_once('|') {
        (app.trim(), text.trim())
    } else {
        ("TextEdit", raw.trim())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_adapters::MockProvider;

    #[tokio::test]
    async fn text_flow_reaches_idle_after_completion() {
        let mut orchestrator = Orchestrator::new(
            MockProvider,
            ToolRuntime::default(),
            VoicePipeline::new(),
            MacOsIntegration::new(),
        );

        let summary = orchestrator
            .handle_text_input("你好")
            .await
            .expect("text flow should succeed");

        assert_eq!(summary.final_state, AvatarState::Idle);
        assert!(!summary.answer.is_empty());
    }
}
