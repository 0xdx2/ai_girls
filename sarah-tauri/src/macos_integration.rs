use crate::state_model::PermissionStatus;
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionSnapshot {
    pub accessibility: PermissionStatus,
    pub microphone: PermissionStatus,
    pub screen_recording: PermissionStatus,
}

#[derive(Debug, Error)]
pub enum MacOsError {
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(&'static str),
    #[error("automation disabled by env (set ENABLE_MACOS_AUTOMATION=1 to enable)")]
    AutomationDisabled,
    #[error("command failed: {0}")]
    CommandFailed(String),
}

#[derive(Debug, Default)]
pub struct MacOsIntegration;

impl MacOsIntegration {
    pub fn new() -> Self {
        Self
    }

    pub fn is_macos() -> bool {
        cfg!(target_os = "macos")
    }

    pub async fn check_permissions(&self) -> PermissionSnapshot {
        // Env-var overrides take priority (useful for CI / non-macOS dev)
        let accessibility = env_override("MACOS_ACCESSIBILITY_GRANTED")
            .unwrap_or_else(|| Self::probe_accessibility().unwrap_or(PermissionStatus::Unknown));

        let microphone = env_override("MACOS_MICROPHONE_GRANTED")
            .unwrap_or_else(|| Self::probe_microphone().unwrap_or(PermissionStatus::Unknown));

        let screen_recording = env_override("MACOS_SCREEN_RECORDING_GRANTED")
            .unwrap_or_else(|| {
                Self::probe_screen_recording().unwrap_or(PermissionStatus::Unknown)
            });

        PermissionSnapshot {
            accessibility,
            microphone,
            screen_recording,
        }
    }

    /// Probe Accessibility by attempting a harmless System Events AppleScript call.
    fn probe_accessibility() -> Option<PermissionStatus> {
        if !Self::is_macos() {
            return None;
        }
        let out = std::process::Command::new("osascript")
            .args([
                "-e",
                "tell application \"System Events\" to get name of first application process whose frontmost is true",
            ])
            .output()
            .ok()?;

        if out.status.success() {
            Some(PermissionStatus::Granted)
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            debug!("accessibility probe stderr: {err}");
            if err.contains("1002") || err.contains("not allowed") || err.contains("not authorized") {
                Some(PermissionStatus::Denied)
            } else {
                Some(PermissionStatus::Unknown)
            }
        }
    }

    /// Probe Microphone permission via a Swift one-liner (AVFoundation).
    fn probe_microphone() -> Option<PermissionStatus> {
        if !Self::is_macos() {
            return None;
        }
        let code = concat!(
            "import AVFoundation; ",
            "let s = AVCaptureDevice.authorizationStatus(for: .audio).rawValue; ",
            "if s == 3 { print(\"Granted\") } else if s == 1 || s == 2 { print(\"Denied\") } else { print(\"Unknown\") }"
        );
        let out = std::process::Command::new("swift")
            .args(["-e", code])
            .output()
            .ok()?;
        parse_swift_line(std::str::from_utf8(&out.stdout).unwrap_or(""))
    }

    /// Probe Screen Recording by attempting to list on-screen windows.
    fn probe_screen_recording() -> Option<PermissionStatus> {
        if !Self::is_macos() {
            return None;
        }
        let code = concat!(
            "import Cocoa; ",
            "let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[CFString: Any]]; ",
            "let ok = list.map { !$0.isEmpty } ?? false; ",
            "print(ok ? \"Granted\" : \"Unknown\")"
        );
        let out = std::process::Command::new("swift")
            .args(["-e", code])
            .output()
            .ok()?;
        parse_swift_line(std::str::from_utf8(&out.stdout).unwrap_or(""))
    }

    pub async fn get_frontmost_app(&self) -> Result<String, MacOsError> {
        if !Self::is_macos() {
            return Err(MacOsError::UnsupportedPlatform("not macOS"));
        }

        let script = "tell application \"System Events\" to get name of first process whose frontmost is true";
        let output = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .await
            .map_err(|e| MacOsError::CommandFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(MacOsError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    pub async fn perform_safe_input(
        &self,
        target_app: &str,
        text: &str,
    ) -> Result<String, MacOsError> {
        if !Self::is_macos() {
            return Err(MacOsError::UnsupportedPlatform("not macOS"));
        }

        if std::env::var("ENABLE_MACOS_AUTOMATION").unwrap_or_default() != "1" {
            return Err(MacOsError::AutomationDisabled);
        }

        // Security: only allow whitelisted applications
        const ALLOWED_APPS: &[&str] = &[
            "Terminal", "iTerm2", "Visual Studio Code", "Code",
            "Finder", "Safari", "TextEdit", "Xcode",
        ];
        if !ALLOWED_APPS.contains(&target_app) {
            return Err(MacOsError::CommandFailed(format!(
                "'{target_app}' is not in the automation whitelist"
            )));
        }

        let escaped = text.replace('"', "\\\"");
        let script = format!(
            "tell application \"{target}\" to activate\n\
             delay 0.3\n\
             tell application \"System Events\" to keystroke \"{text}\"",
            target = target_app,
            text = escaped
        );

        let output = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .await
            .map_err(|e| MacOsError::CommandFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(MacOsError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(format!("input sent to {target_app}"))
    }
}

#[allow(dead_code)]
fn parse_status_env(name: &str) -> PermissionStatus {
    env_override(name).unwrap_or(PermissionStatus::Unknown)
}

fn env_override(var: &str) -> Option<PermissionStatus> {
    match std::env::var(var).ok().as_deref() {
        Some("1") | Some("true") | Some("TRUE") | Some("Granted") | Some("granted") => {
            Some(PermissionStatus::Granted)
        }
        Some("0") | Some("false") | Some("FALSE") | Some("Denied") | Some("denied") => {
            Some(PermissionStatus::Denied)
        }
        _ => None,
    }
}

fn parse_swift_line(stdout: &str) -> Option<PermissionStatus> {
    match stdout.trim() {
        "Granted" => Some(PermissionStatus::Granted),
        "Denied" => Some(PermissionStatus::Denied),
        "Unknown" => Some(PermissionStatus::Unknown),
        other => {
            debug!("unexpected swift output: {other:?}");
            None
        }
    }
}
