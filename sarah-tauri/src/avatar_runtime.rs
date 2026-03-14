use std::time::{Duration, Instant};

use crate::state_model::{AvatarActivity, AvatarState, DomainEvent};

pub struct AvatarStateMachine {
    current: AvatarState,
    /// Fine-grained overlay updated on every event.
    activity: AvatarActivity,
    min_state_duration: Duration,
    last_transition: Instant,
    /// Number of `ModelThinkingChunk` events seen for the current task.
    thinking_chunk_count: u8,
    /// Total todos declared by the current plan.
    plan_total: usize,
    /// Todos completed so far.
    todos_done: usize,
}

impl AvatarStateMachine {
    pub fn new(min_state_duration: Duration) -> Self {
        Self {
            current: AvatarState::Idle,
            activity: AvatarActivity::Idle,
            min_state_duration,
            last_transition: Instant::now(),
            thinking_chunk_count: 0,
            plan_total: 0,
            todos_done: 0,
        }
    }

    pub fn current(&self) -> AvatarState {
        self.current
    }

    /// Returns the current fine-grained activity (updated each `reduce` call).
    pub fn current_activity(&self) -> AvatarActivity {
        self.activity.clone()
    }

    /// Drive the FSM with an incoming domain event.
    /// Returns the new (or unchanged) `AvatarState`.
    /// The activity is also updated internally; callers may read it via
    /// `current_activity()` after this call.
    #[allow(clippy::too_many_lines)]
    pub fn reduce(&mut self, event: &DomainEvent) -> AvatarState {
        // ── Step 1: update fine-grained activity ──────────────────────────────
        match event {
            DomainEvent::UserInputReceived { .. } => {
                self.thinking_chunk_count = 0;
                self.plan_total = 0;
                self.todos_done = 0;
                self.activity = AvatarActivity::Idle;
            }
            DomainEvent::AgentThinkingStarted { .. } => {
                self.thinking_chunk_count = 0;
                self.activity = AvatarActivity::ThinkingLight;
            }
            DomainEvent::AgentThinkingFinished { .. } => {
                self.thinking_chunk_count = 0;
                // keep whatever light/deep activity was set; state transition handles rest
            }
            DomainEvent::ModelThinkingChunk { .. } => {
                self.thinking_chunk_count = self.thinking_chunk_count.saturating_add(1);
                self.activity = if self.thinking_chunk_count >= 3 {
                    AvatarActivity::ThinkingDeep
                } else {
                    AvatarActivity::ThinkingLight
                };
            }
            DomainEvent::AgentPlanCreated { todos, .. } => {
                self.plan_total = todos.len();
                self.todos_done = 0;
                self.activity = AvatarActivity::Planning;
            }
            DomainEvent::AgentTodoUpdated { done, .. } => {
                if *done {
                    self.todos_done = self.todos_done.saturating_add(1);
                }
                self.activity = AvatarActivity::TodoProgress;
            }
            DomainEvent::AgentCodeGenerated { .. } => {
                self.activity = AvatarActivity::GeneratingCode;
            }
            DomainEvent::SkillInvoked { .. } => {
                self.activity = AvatarActivity::InvokingSkill;
            }
            DomainEvent::ToolCallStarted { .. } => {
                self.activity = AvatarActivity::UsingTool;
            }
            DomainEvent::ToolCallFinished { success, .. } => {
                self.activity = if *success {
                    AvatarActivity::Celebrating
                } else {
                    AvatarActivity::Idle
                };
            }
            DomainEvent::VoicePlaybackStarted => {
                self.activity = AvatarActivity::Speaking;
            }
            DomainEvent::TaskCompleted { .. } | DomainEvent::VoicePlaybackFinished => {
                self.activity = AvatarActivity::Celebrating;
            }
            DomainEvent::ErrorOccurred { .. } => {
                self.activity = AvatarActivity::Idle;
            }
            _ => {}
        }

        // ── Step 2: compute target AvatarState ────────────────────────────────
        let target = match event {
            DomainEvent::ErrorOccurred { .. } => AvatarState::Error,
            DomainEvent::UserInputReceived { .. } => AvatarState::Listening,
            DomainEvent::AsrFinal { .. }
            | DomainEvent::AgentThinkingStarted { .. }
            | DomainEvent::ModelThinkingChunk { .. } => AvatarState::Thinking,
            DomainEvent::AgentAnswerReady { .. } => AvatarState::Speaking,
            DomainEvent::ToolCallStarted { .. }
            | DomainEvent::SkillInvoked { .. }
            | DomainEvent::AgentCodeGenerated { .. }
            | DomainEvent::AgentPlanCreated { .. }
            | DomainEvent::AgentTodoUpdated { .. }
            | DomainEvent::SystemActionRequested { .. } => AvatarState::Working,
            DomainEvent::ToolCallFinished { success, .. }
            | DomainEvent::SystemActionExecuted { success, .. } => {
                if *success {
                    AvatarState::Success
                } else {
                    AvatarState::Error
                }
            }
            DomainEvent::VoicePlaybackFinished
            | DomainEvent::TaskCompleted { .. } => AvatarState::Idle,
            _ => self.current,
        };

        // Error always preempts debounce
        if target == AvatarState::Error {
            return self.transition(target);
        }

        // Don't re-trigger Listening rapidly from repeated input events
        let should_debounce_duplicate_listening = matches!(event, DomainEvent::UserInputReceived { .. })
            && self.current == AvatarState::Listening
            && target == AvatarState::Listening
            && self.last_transition.elapsed() < self.min_state_duration;

        if should_debounce_duplicate_listening {
            return self.current;
        }

        self.transition(target)
    }

    /// CSS class for the base avatar state (e.g. `"avatar_thinking"`).
    pub fn visual_hint(state: AvatarState) -> &'static str {
        match state {
            AvatarState::Idle => "avatar_idle",
            AvatarState::Listening => "avatar_listening",
            AvatarState::Thinking => "avatar_thinking",
            AvatarState::Speaking => "avatar_speaking",
            AvatarState::Working => "avatar_working",
            AvatarState::Waiting => "avatar_waiting",
            AvatarState::Error => "avatar_error",
            AvatarState::Success => "avatar_success",
        }
    }

    fn transition(&mut self, next: AvatarState) -> AvatarState {
        if self.current != next {
            self.current = next;
            self.last_transition = Instant::now();
        }
        self.current
    }
}

impl Default for AvatarStateMachine {
    fn default() -> Self {
        Self::new(Duration::from_millis(300))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enters_thinking_on_agent_event() {
        let mut fsm = AvatarStateMachine::default();
        let state = fsm.reduce(&DomainEvent::AgentThinkingStarted { task_id: 1 });
        assert_eq!(state, AvatarState::Thinking);
    }

    #[test]
    fn thinking_chunk_escalates_to_deep() {
        let mut fsm = AvatarStateMachine::default();
        fsm.reduce(&DomainEvent::AgentThinkingStarted { task_id: 1 });
        assert_eq!(fsm.current_activity(), AvatarActivity::ThinkingLight);
        for _ in 0..3 {
            fsm.reduce(&DomainEvent::ModelThinkingChunk {
                task_id: 1,
                text: "step".into(),
            });
        }
        assert_eq!(fsm.current_activity(), AvatarActivity::ThinkingDeep);
    }

    #[test]
    fn plan_sets_planning_activity() {
        let mut fsm = AvatarStateMachine::default();
        fsm.reduce(&DomainEvent::AgentPlanCreated {
            task_id: 1,
            todos: vec!["step 1".into(), "step 2".into()],
        });
        assert_eq!(fsm.current_activity(), AvatarActivity::Planning);
        assert_eq!(fsm.current(), AvatarState::Working);
    }

    #[test]
    fn tool_call_sets_using_tool_activity() {
        let mut fsm = AvatarStateMachine::default();
        fsm.reduce(&DomainEvent::ToolCallStarted {
            tool: "terminal".into(),
            action: "ls".into(),
            category: crate::state_model::ToolCategory::Terminal,
        });
        assert_eq!(fsm.current_activity(), AvatarActivity::UsingTool);
        assert_eq!(fsm.current(), AvatarState::Working);
    }
}
