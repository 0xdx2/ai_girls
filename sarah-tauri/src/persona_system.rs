use serde::{Deserialize, Serialize};

/// The role identity an AI agent is currently playing.
/// Each role maps to a distinct visual "costume" theme, accent colour and icon
/// that the frontend renders on the avatar costume badge and props tray.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    #[default]
    /// Default conversational assistant — casual outfit.
    Assistant,
    /// Strategic planner — military/tactician aesthetic.
    Planner,
    /// Code writer — hacker/tech aesthetic with holographic screens.
    Coder,
    /// Web researcher — explorer aesthetic with a spy glass.
    Researcher,
    /// Multi-agent orchestrator — conductor aesthetic with a baton.
    Orchestrator,
    /// Security/audit role — detective aesthetic.
    Security,
    /// Data analyst — scientist aesthetic with data charts.
    Analyst,
    /// Freestyle role with custom label.
    Custom(String),
}

impl AgentRole {
    /// Infer a role from a system-prompt or CLI command string.
    pub fn from_context(context: &str) -> Self {
        let lower = context.to_lowercase();
        if lower.contains("plan") || lower.contains("task") || lower.contains("todo") {
            AgentRole::Planner
        } else if lower.contains("code") || lower.contains("rust") || lower.contains("python")
            || lower.contains("javascript") || lower.contains("typescript")
        {
            AgentRole::Coder
        } else if lower.contains("search") || lower.contains("browse") || lower.contains("research") {
            AgentRole::Researcher
        } else if lower.contains("orchestrat") || lower.contains("agent") || lower.contains("multi") {
            AgentRole::Orchestrator
        } else if lower.contains("security") || lower.contains("audit") || lower.contains("vuln") {
            AgentRole::Security
        } else if lower.contains("analys") || lower.contains("data") || lower.contains("metric") {
            AgentRole::Analyst
        } else {
            AgentRole::Assistant
        }
    }

    pub fn label(&self) -> &str {
        match self {
            AgentRole::Assistant => "Assistant",
            AgentRole::Planner => "Planner",
            AgentRole::Coder => "Coder",
            AgentRole::Researcher => "Researcher",
            AgentRole::Orchestrator => "Orchestrator",
            AgentRole::Security => "Security",
            AgentRole::Analyst => "Analyst",
            AgentRole::Custom(s) => s.as_str(),
        }
    }
}

/// Complete visual identity for an avatar when playing a specific role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaProfile {
    pub role: AgentRole,
    /// Human-readable display name shown in the persona badge.
    pub display_name: String,
    /// Primary CSS accent colour (hex, e.g. "#7c9cff").
    pub accent_color: &'static str,
    /// Secondary CSS colour for gradients / glows.
    pub glow_color: &'static str,
    /// Emoji icon representing this role in the badge and props tray.
    pub icon: &'static str,
    /// Name of the `Live2D` expression to apply (empty = default).
    pub live2d_expression: &'static str,
    /// Name of the `Live2D` idle motion group for this role.
    pub live2d_motion: &'static str,
    /// Descriptive costume tag displayed in the UI.
    pub costume_tag: &'static str,
}

impl PersonaProfile {
    /// Create a profile for the given role using built-in defaults.
    #[allow(clippy::needless_pass_by_value)]
    pub fn for_role(role: AgentRole) -> Self {
        let display_name = role.label().to_owned();
        match role {
            AgentRole::Assistant => Self {
                role: AgentRole::Assistant,
                display_name,
                accent_color: "#7c9cff",
                glow_color: "rgba(124,156,255,0.4)",
                icon: "🌸",
                live2d_expression: "happy",
                live2d_motion: "Idle",
                costume_tag: "Casual",
            },
            AgentRole::Planner => Self {
                role: AgentRole::Planner,
                display_name,
                accent_color: "#56d4c9",
                glow_color: "rgba(86,212,201,0.4)",
                icon: "📋",
                live2d_expression: "focused",
                live2d_motion: "Thinking",
                costume_tag: "Tactician",
            },
            AgentRole::Coder => Self {
                role: AgentRole::Coder,
                display_name,
                accent_color: "#3cc8dc",
                glow_color: "rgba(60,200,220,0.4)",
                icon: "💻",
                live2d_expression: "concentrated",
                live2d_motion: "Typing",
                costume_tag: "Hacker",
            },
            AgentRole::Researcher => Self {
                role: AgentRole::Researcher,
                display_name,
                accent_color: "#f0a060",
                glow_color: "rgba(240,160,96,0.4)",
                icon: "🔍",
                live2d_expression: "curious",
                live2d_motion: "Searching",
                costume_tag: "Explorer",
            },
            AgentRole::Orchestrator => Self {
                role: AgentRole::Orchestrator,
                display_name,
                accent_color: "#c084fc",
                glow_color: "rgba(192,132,252,0.4)",
                icon: "🎼",
                live2d_expression: "commanding",
                live2d_motion: "Conducting",
                costume_tag: "Conductor",
            },
            AgentRole::Security => Self {
                role: AgentRole::Security,
                display_name,
                accent_color: "#f43f5e",
                glow_color: "rgba(244,63,94,0.4)",
                icon: "🔐",
                live2d_expression: "alert",
                live2d_motion: "Scanning",
                costume_tag: "Detective",
            },
            AgentRole::Analyst => Self {
                role: AgentRole::Analyst,
                display_name,
                accent_color: "#34d399",
                glow_color: "rgba(52,211,153,0.4)",
                icon: "📊",
                live2d_expression: "analytical",
                live2d_motion: "Reading",
                costume_tag: "Scientist",
            },
            AgentRole::Custom(ref label) => Self {
                role: AgentRole::Custom(label.clone()),
                display_name,
                accent_color: "#a8a8c0",
                glow_color: "rgba(168,168,192,0.4)",
                icon: "✨",
                live2d_expression: "happy",
                live2d_motion: "Idle",
                costume_tag: "Custom",
            },
        }
    }
}

/// Manages the currently active persona and tracks role history.
#[derive(Debug)]
pub struct PersonaManager {
    current: PersonaProfile,
    history: Vec<AgentRole>,
}

impl Default for PersonaManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PersonaManager {
    pub fn new() -> Self {
        Self {
            current: PersonaProfile::for_role(AgentRole::Assistant),
            history: vec![AgentRole::Assistant],
        }
    }

    /// Switch to a new role and return the new profile.
    pub fn switch_to(&mut self, role: AgentRole) -> &PersonaProfile {
        if self.current.role != role {
            self.history.push(role.clone());
            self.current = PersonaProfile::for_role(role);
        }
        &self.current
    }

    /// Infer and switch role from task context text.
    pub fn infer_and_switch(&mut self, context: &str) -> &PersonaProfile {
        let role = AgentRole::from_context(context);
        self.switch_to(role)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_coder_from_rust() {
        let role = AgentRole::from_context("write a rust function");
        assert_eq!(role, AgentRole::Coder);
    }

    #[test]
    fn infer_researcher_from_browse() {
        let role = AgentRole::from_context("browse and search the web");
        assert_eq!(role, AgentRole::Researcher);
    }

    #[test]
    fn persona_profile_has_icon() {
        let p = PersonaProfile::for_role(AgentRole::Coder);
        assert_eq!(p.icon, "💻");
        assert_eq!(p.costume_tag, "Hacker");
    }

    #[test]
    fn manager_switch() {
        let mut mgr = PersonaManager::new();
        let p = mgr.switch_to(AgentRole::Planner);
        assert_eq!(p.role, AgentRole::Planner);
        assert_eq!(mgr.history.len(), 2);
    }
}
