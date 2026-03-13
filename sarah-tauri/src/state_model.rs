use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AvatarState {
    #[default]
    Idle,
    Listening,
    Thinking,
    Speaking,
    Working,
    Waiting,
    Error,
    Success,
}

/// Fine-grained activity the avatar is performing within its base state.
/// Layered on top of `AvatarState` so the frontend applies a second CSS class
/// and triggers matching Live2D motions / expressions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AvatarActivity {
    #[default]
    Idle,
    /// Model is doing lightweight reasoning (few CoT chunks seen).
    ThinkingLight,
    /// Model is in deep chain-of-thought (many reasoning chunks).
    ThinkingDeep,
    /// Model produced a structured task plan / todo list.
    Planning,
    /// Model is executing items from the plan step by step.
    TodoProgress,
    /// A tool (terminal, browser, filesystem …) is being called.
    UsingTool,
    /// A named skill / built-in capability is being invoked.
    InvokingSkill,
    /// Code was detected in the model's output — avatar shows "coding" pose.
    GeneratingCode,
    /// TTS playback is in progress.
    Speaking,
    /// Task just completed — brief celebration before returning to Idle.
    Celebrating,
}

impl AvatarActivity {
    /// CSS class suffix for this activity (applied alongside the state class).
    pub fn hint(&self) -> &'static str {
        match self {
            AvatarActivity::Idle => "activity_idle",
            AvatarActivity::ThinkingLight => "activity_thinking_light",
            AvatarActivity::ThinkingDeep => "activity_thinking_deep",
            AvatarActivity::Planning => "activity_planning",
            AvatarActivity::TodoProgress => "activity_todo",
            AvatarActivity::UsingTool => "activity_tool",
            AvatarActivity::InvokingSkill => "activity_skill",
            AvatarActivity::GeneratingCode => "activity_code",
            AvatarActivity::Speaking => "activity_speaking",
            AvatarActivity::Celebrating => "activity_celebrating",
        }
    }
}

/// Classifies which kind of tool the avatar is currently using.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    Terminal,
    Browser,
    Filesystem,
    Code,
    Search,
    Skill,
    System,
}

impl ToolCategory {

    #[allow(dead_code)]
    pub fn from_tool_name(name: &str) -> Self {
        match name {
            "terminal" => ToolCategory::Terminal,
            "filesystem" => ToolCategory::Filesystem,
            s if s.starts_with("browse") || s.starts_with("http") => ToolCategory::Browser,
            s if s.contains("search") || s.contains("grep") => ToolCategory::Search,
            s if s.contains("code") || s.contains("git") => ToolCategory::Code,
            "skill" => ToolCategory::Skill,
            _ => ToolCategory::System,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionType {
    Accessibility,
    Microphone,
    ScreenRecording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionStatus {
    Granted,
    Denied,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: u64,
    pub prompt: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsageMetric {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisemeFrame {
    pub start_ms: u64,
    pub end_ms: u64,
    pub viseme: String,
    pub intensity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DomainEvent {
    UserInputReceived { text: String },
    AsrFinal { text: String },
    AgentThinkingStarted { task_id: u64 },
    AgentThinkingFinished { task_id: u64 },
    /// One chunk of the model's chain-of-thought / extended thinking.
    ModelThinkingChunk { task_id: u64, text: String },
    AgentAnswerReady { task_id: u64, answer: String },
    /// Model outlined a structured task plan — avatar enters Planning pose.
    AgentPlanCreated { task_id: u64, todos: Vec<String> },
    /// One todo item was added or ticked off.
    AgentTodoUpdated { task_id: u64, index: usize, title: String, done: bool },
    /// Model generated code detected in the response.
    AgentCodeGenerated { task_id: u64, language: String, preview: String },
    /// A named skill / capability was invoked by the model.
    SkillInvoked { task_id: u64, skill: String },
    ToolCallStarted { tool: String, action: String, category: ToolCategory },
    ToolCallFinished { tool: String, success: bool, output: String },
    VoicePlaybackStarted,
    VoicePlaybackFinished,
    LipSyncFramesGenerated { frame_count: usize },
    TokenUsageUpdated { metric: TokenUsageMetric },
    PermissionRequired { permission: PermissionType },
    PermissionGranted { permission: PermissionType },
    PermissionDenied { permission: PermissionType },
    SystemActionRequested { action: String, target: String },
    SystemActionExecuted { action: String, target: String, success: bool },
    /// Emitted whenever the avatar FSM changes state.
    AvatarStateChanged { from: AvatarState, to: AvatarState, activity: AvatarActivity },
    /// Emitted when the active agent role changes.
    PersonaChanged {
        role: String,
        display_name: String,
        icon: String,
        accent_color: String,
        glow_color: String,
        costume_tag: String,
        live2d_expression: String,
        live2d_motion: String,
    },
    /// Emitted when a named skill is unlocked / first seen.
    SkillDiscovered { skill: String, icon: String, description: String },
    TaskCompleted { task_id: u64 },
    ErrorOccurred { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_state_default_is_idle() {
        assert_eq!(AvatarState::default(), AvatarState::Idle);
    }
}
