use std::{
    fs, io,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// How long a conversation survives without a finished reply.
pub const SESSION_TIMEOUT: Duration = Duration::from_secs(5 * 60);

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
}

/// Everything persisted between launches, stored as `state.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub session_id: Option<String>,
    /// Unix seconds when the last reply finished.
    pub last_activity: Option<u64>,
    pub transcript: Vec<Message>,
}

impl State {
    /// Loads saved state, falling back to an empty conversation if the file is
    /// missing or unreadable.
    pub fn load() -> Self {
        fs::read(state_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Writes to a temporary file and renames it so a crash never leaves a
    /// half-written state file behind.
    pub fn save(&self) -> io::Result<()> {
        let path = state_path();
        fs::create_dir_all(support_dir())?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, path)
    }

    pub fn push(&mut self, role: Role, text: impl Into<String>) {
        self.transcript.push(Message {
            role,
            text: text.into(),
        });
    }

    pub fn touch(&mut self) {
        self.last_activity = Some(now());
    }

    /// Drops the transcript and session once the timeout has passed since the
    /// last finished reply. Returns whether anything was cleared.
    pub fn expire_if_stale(&mut self) -> bool {
        let stale = match self.last_activity {
            Some(last) => now().saturating_sub(last) >= SESSION_TIMEOUT.as_secs(),
            None => !self.transcript.is_empty() || self.session_id.is_some(),
        };
        if stale {
            *self = State::default();
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

/// `~/Library/Application Support/claude-companion`
pub fn support_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Library/Application Support/claude-companion")
}

fn state_path() -> PathBuf {
    support_dir().join("state.json")
}

/// Fixed working directory for `claude`, which stores sessions per project
/// directory: `--resume` only finds sessions started from the same place.
pub fn work_dir() -> io::Result<PathBuf> {
    let dir = support_dir().join("workdir");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!state.expire_if_stale());
        assert_eq!(state.session_id.as_deref(), Some("abc"));
        assert_eq!(state.transcript.len(), 1);
    }

    #[test]
    fn clears_conversation_after_timeout() {
        let mut state = conversation(Some(now() - SESSION_TIMEOUT.as_secs()));
        assert!(state.expire_if_stale());
        assert!(state.session_id.is_none());
        assert!(state.transcript.is_empty());
    }

    #[test]
    fn clears_conversation_that_never_got_a_reply() {
        let mut state = conversation(None);
        assert!(state.expire_if_stale());
    }

    #[test]
    fn empty_state_is_not_stale() {
        assert!(!State::default().expire_if_stale());
    }
}
