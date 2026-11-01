//! Runs a long-lived `claude -p` process with `stream-json` on both ends, so
//! the CLI's startup cost is paid once per conversation rather than once per
//! message, and turns its output into [`Event`]s.

use std::{
    io::{self, Write},
    path::PathBuf,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};
use smol::{
    Unblock,
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    stream::StreamExt,
};

use crate::{settings::Settings, storage};

#[derive(Debug, PartialEq)]
pub enum Event {
    /// A new assistant message began; the previous one, if any, ended in a
    /// tool call.
    MessageStarted,
    TextDelta(String),
    ToolStarted(String),
    /// An assistant message ended. `last` is false when Claude stopped to
    /// use a tool and will carry on in another message.
    MessageEnded {
        last: bool,
    },
    /// The turn is fully settled. This arrives a second or two after the
    /// last `MessageEnded`, once the CLI's post-turn work is done.
    Finished {
        session_id: Option<String>,
        outcome: Result<String, String>,
    },
}

/// The running CLI. Dropping it kills the process.
pub struct Process {
    child: Child,
    stdin: ChildStdin,
}

/// The process's output, consumed by [`Output::read`].
pub struct Output {
    stdout: Unblock<ChildStdout>,
    stderr: Unblock<ChildStderr>,
}

/// Starts `claude`, resuming `session_id` when given. It waits on stdin, so
/// starting it before the user has finished typing hides the startup time.
pub fn start(settings: &Settings, session_id: Option<&str>) -> Result<(Process, Output), String> {
    let tools = settings.tools().join(",");
    let mut command = Command::new(executable());
    command
        .current_dir(storage::work_dir().map_err(|error| error.to_string())?)
        .args(["-p", "--verbose"])
        .args(["--input-format", "stream-json"])
        .args(["--output-format", "stream-json"])
        .arg("--include-partial-messages")
        // An empty --tools disables every built-in tool. --allowedTools
        // pre-approves the enabled ones, since nobody can answer a prompt.
        .args(["--tools", &tools])
        .arg("--strict-mcp-config")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !tools.is_empty() {
        command.args(["--allowedTools", &tools]);
    }
    if let Some(model) = settings.model.flag() {
        command.args(["--model", model]);
    }
    if let Some(effort) = settings.effort.flag() {
        command.args(["--effort", effort]);
    }
    if let Some(id) = session_id {
        command.args(["--resume", id]);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("Couldn't start claude: {error}"))?;
    let stdin = child.stdin.take().expect("stdin is piped");
    let output = Output {
        stdout: Unblock::new(child.stdout.take().expect("stdout is piped")),
        stderr: Unblock::new(child.stderr.take().expect("stderr is piped")),
    };
    Ok((Process { child, stdin }, output))
}

impl Process {
    /// Queues a user message. The CLI handles one turn at a time, so a
    /// message sent before the previous turn settles waits for it.
    pub fn send(&mut self, prompt: &str) -> io::Result<()> {
        let message = json!({
            "type": "user",
            "message": { "role": "user", "content": prompt },
        });
        writeln!(self.stdin, "{message}")?;
        self.stdin.flush()
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

impl Output {
    /// Calls `on_event` for each parsed event until the process exits or
    /// `on_event` returns `false`, then returns whatever it wrote to stderr.
    /// stderr is drained alongside stdout so neither pipe can fill and block.
    pub async fn read(self, mut on_event: impl FnMut(Event) -> bool) -> String {
        let Output { stdout, mut stderr } = self;
        let read_events = async {
            let mut lines = BufReader::new(stdout).lines();
            while let Some(Ok(line)) = lines.next().await {
                if let Some(event) = parse_line(&line)
                    && !on_event(event)
                {
                    break;
                }
            }
        };
        let read_errors = async {
            let mut output = String::new();
            stderr.read_to_string(&mut output).await.ok();
            output
        };
        let ((), errors) = smol::future::zip(read_events, read_errors).await;
        errors.trim().to_string()
    }
}

fn parse_line(line: &str) -> Option<Event> {
    let value: Value = serde_json::from_str(line).ok()?;
    match value["type"].as_str()? {
        "stream_event" => parse_stream_event(&value["event"]),
        "result" => {
            let text = value["result"].as_str().unwrap_or_default().to_string();
            let failed = value["is_error"].as_bool().unwrap_or(false)
                || value["subtype"].as_str() != Some("success");
            let outcome = if failed {
                Err(if text.is_empty() {
                    value["subtype"].as_str().unwrap_or("error").to_string()
                } else {
                    text
                })
            } else {
                Ok(text)
            };
            Some(Event::Finished {
                session_id: value["session_id"].as_str().map(str::to_string),
                outcome,
            })
        }
        _ => None,
    }
}

fn parse_stream_event(event: &Value) -> Option<Event> {
    match event["type"].as_str()? {
        "message_start" => Some(Event::MessageStarted),
        "content_block_delta" if event["delta"]["type"] == "text_delta" => Some(Event::TextDelta(
            event["delta"]["text"].as_str()?.to_string(),
        )),
        "content_block_start" => {
            let block = &event["content_block"];
            match block["type"].as_str()? {
                "tool_use" | "server_tool_use" => {
                    Some(Event::ToolStarted(block["name"].as_str()?.to_string()))
                }
                _ => None,
            }
        }
        "message_delta" => {
            let stop_reason = event["delta"]["stop_reason"].as_str()?;
            Some(Event::MessageEnded {
                last: !matches!(stop_reason, "tool_use" | "pause_turn"),
            })
        }
        _ => None,
    }
}

/// Apps launched from Finder don't inherit the shell's `PATH`, so look in the
/// usual install locations before falling back to a `PATH` lookup.
/// `CLAUDE_COMPANION_CLI` overrides all of them.
fn executable() -> PathBuf {
    if let Some(path) = std::env::var_os("CLAUDE_COMPANION_CLI") {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    [
        home.join(".local/bin/claude"),
        home.join(".claude/local/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .unwrap_or_else(|| PathBuf::from("claude"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_event(event: &str) -> String {
        format!(r#"{{"type":"stream_event","event":{event}}}"#)
    }

    #[test]
    fn parses_text_delta() {
        let line = stream_event(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
        );
        assert_eq!(parse_line(&line), Some(Event::TextDelta("hi".into())));
    }

    #[test]
    fn parses_tool_start() {
        let line = stream_event(
            r#"{"type":"content_block_start","content_block":{"type":"server_tool_use","name":"web_search"}}"#,
        );
        assert_eq!(
            parse_line(&line),
            Some(Event::ToolStarted("web_search".into()))
        );
    }

    #[test]
    fn parses_message_end() {
        let end = |reason: &str| {
            parse_line(&stream_event(&format!(
                r#"{{"type":"message_delta","delta":{{"stop_reason":"{reason}"}}}}"#
            )))
        };
        assert_eq!(end("end_turn"), Some(Event::MessageEnded { last: true }));
        assert_eq!(end("max_tokens"), Some(Event::MessageEnded { last: true }));
        assert_eq!(end("tool_use"), Some(Event::MessageEnded { last: false }));
    }

    #[test]
    fn parses_successful_result() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"hi there","session_id":"abc"}"#;
        assert_eq!(
            parse_line(line),
            Some(Event::Finished {
                session_id: Some("abc".into()),
                outcome: Ok("hi there".into()),
            })
        );
    }

    #[test]
    fn parses_failed_result() {
        let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"session_id":"abc"}"#;
        assert_eq!(
            parse_line(line),
            Some(Event::Finished {
                session_id: Some("abc".into()),
                outcome: Err("error_during_execution".into()),
            })
        );
    }

    #[test]
    fn ignores_other_lines() {
        assert!(parse_line(r#"{"type":"system","subtype":"init"}"#).is_none());
        assert!(parse_line("not json").is_none());
    }
}
