use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{attachments, storage};

const FILE: &str = "state.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub text: String,
    /// Attached images, as copies in the attachments folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<PathBuf>,
}

/// The conversation, persisted between launches as `state.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub session_id: Option<String>,
    /// Unix seconds when the last reply finished.
    pub last_activity: Option<u64>,
    pub transcript: Vec<Message>,
}

impl State {
    pub fn load() -> Self {
        storage::load(FILE)
    }

    pub fn save(&self) -> std::io::Result<()> {
        storage::save(FILE, self)
    }

    pub fn push(&mut self, role: Role, text: impl Into<String>) {
        self.push_with_images(role, text, Vec::new());
    }

    pub fn push_with_images(&mut self, role: Role, text: impl Into<String>, images: Vec<PathBuf>) {
        self.transcript.push(Message {
            role,
            text: text.into(),
            images,
        });
    }

    /// Starts a new conversation, deleting the old one's attachments.
    pub fn clear(&mut self) {
        let old = std::mem::take(self);
        attachments::remove(
            old.transcript
                .into_iter()
                .flat_map(|message| message.images),
        );
    }

    pub fn touch(&mut self) {
        self.last_activity = Some(now());
    }

    /// Whether `timeout` has passed since the last finished reply. A
    /// conversation that never got a reply counts as stale.
    pub fn is_stale(&self, timeout: Duration) -> bool {
        match self.last_activity {
            Some(last) => now().saturating_sub(last) >= timeout.as_secs(),
            None => !self.transcript.is_empty() || self.session_id.is_some(),
        }
    }

    /// Drops the transcript and session if stale. Returns whether it did.
    pub fn expire_if_stale(&mut self, timeout: Duration) -> bool {
        let stale = self.is_stale(timeout);
        if stale {
            self.clear();
        }
        stale
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(5 * 60);

    fn conversation(last_activity: Option<u64>) -> State {
        let mut state = State {
            session_id: Some("abc".into()),
            last_activity,
            transcript: Vec::new(),
        };
        state.push(Role::User, "hello");
        state
    }

    #[test]
    fn keeps_recent_conversation() {
        let mut state = conversation(Some(now() - 60));
        assert!(!state.expire_if_stale(TIMEOUT));
        assert_eq!(state.session_id.as_deref(), Some("abc"));
        assert_eq!(state.transcript.len(), 1);
    }

    #[test]
    fn clears_conversation_after_timeout() {
        let mut state = conversation(Some(now() - TIMEOUT.as_secs()));
        assert!(state.expire_if_stale(TIMEOUT));
        assert!(state.session_id.is_none());
        assert!(state.transcript.is_empty());
    }

    #[test]
    fn clears_conversation_that_never_got_a_reply() {
        let mut state = conversation(None);
        assert!(state.expire_if_stale(TIMEOUT));
    }

    #[test]
    fn empty_state_is_not_stale() {
        assert!(!State::default().expire_if_stale(TIMEOUT));
    }
}
