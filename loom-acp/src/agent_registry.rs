//! Agent registry: maps loom agent profiles to ACP session modes.
//!
//! Delegates all agent discovery and loading to loom core library.
//! Each ACP Session Mode maps 1:1 to a Loom Agent Profile.

use agent_client_protocol::{SessionMode, SessionModeId, SessionModeState};
use loom::{list_available_profiles, resolve_profile, AgentProfile, ProfileSummary};

#[derive(Debug, Clone)]
pub struct AgentRegistry {
    profiles: Vec<ProfileSummary>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        let profiles = list_available_profiles();
        Self { profiles }
    }

    pub fn to_session_modes(&self) -> Vec<SessionMode> {
        self.profiles
            .iter()
            .map(|p| self.profile_to_session_mode(p))
            .collect()
    }

    pub fn to_session_mode_state(&self, current_mode_id: &str) -> SessionModeState {
        SessionModeState::new(SessionModeId::new(current_mode_id), self.to_session_modes())
    }

    pub fn default_mode_id(&self) -> &'static str {
        "dev"
    }

    pub fn get_agent_config(&self, mode_id: &str) -> Option<AgentProfile> {
        resolve_profile(mode_id).ok()
    }

    pub fn mode_exists(&self, mode_id: &str) -> bool {
        self.profiles.iter().any(|p| p.name == mode_id)
    }

    pub fn resolve_agent_name(&self, mode_id: &str) -> String {
        mode_id.to_string()
    }

    fn profile_to_session_mode(&self, profile: &ProfileSummary) -> SessionMode {
        SessionMode::new(
            SessionModeId::new(profile.name.as_str()),
            self.display_name(&profile.name),
        )
        .description(
            profile
                .description
                .clone()
                .unwrap_or_else(|| format!("{} agent", profile.name)),
        )
    }

    fn display_name(&self, id: &str) -> String {
        match id {
            "dev" => "Default".to_string(),
            "agent-builder" => "Agent Builder".to_string(),
            other => other.to_string(),
        }
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
