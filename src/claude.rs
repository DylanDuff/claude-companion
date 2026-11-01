//! Drives `claude -p` and turns its `stream-json` output into [`Event`]s.

use std::{path::PathBuf, process::Stdio};

use serde_json::Value;
use smol::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    stream::StreamExt,
};

use crate::state;

/// Built-in tools the companion may use. Everything else, MCP servers
/// included, is switched off.
const TOOLS: &str = "WebSearch,WebFetch";

#[derive(Debug)]
pub enum Event {
    /// A new assistant message began; the previous one, if any, ended in a
    /// tool call.
    MessageStarted,
    TextDelta(String),
    ToolStarted(String),
    Finished {
        session_id: Option<String>,
        outcome: Result<String, String>,
    },
}

/// Sends `prompt` to Claude, resuming `session_id` when given, and calls
/// `on_event` for each parsed event until the process exits. Returning
/// `false` from `on_event` stops reading.
///
/// Errors are process-level failures (could not start, crashed); a reply
/// that Claude itself reports as failed arrives as [`Event::Finished`].
pub async fn send(
    prompt: &str,
    session_id: Option<&str>,
    mut on_event: impl FnMut(Event) -> bool,
) -> Result<(), String> {
    let mut command = Command::new(executable());
    command
        .current_dir(state::work_dir().map_err(|error| error.to_string())?)
        .args(["-p", "--output-format", "stream-json", "--verbose"])
        .arg("--include-partial-messages")
        .args(["--tools", TOOLS, "--allowedTools", TOOLS])
        .arg("--strict-mcp-config")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(id) = session_id {
        command.args(["--resume", id]);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("Couldn't start claude: {error}"))?;

    // The prompt goes over stdin so text starting with `-` is never read as
    // a flag.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .await
            .map_err(|error| error.to_string())?;
    }

    let stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");

    // stderr is drained alongside stdout so a chatty process can't block on
    // a full pipe.
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

    let status = child.status().await.map_err(|error| error.to_string())?;
    if status.success() {
        return Ok(());
    }
    let errors = errors.trim();
    Err(if errors.is_empty() {
        format!("claude exited with {status}")
    } else {
        errors.to_string()
    })
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

    #[test]
    fn parses_text_delta() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}}"#;
        assert!(matches!(parse_line(line), Some(Event::TextDelta(text)) if text == "hi"));
    }

    #[test]
    fn parses_tool_start() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_start","content_block":{"type":"server_tool_use","name":"web_search"}}}"#;
        assert!(matches!(parse_line(line), Some(Event::ToolStarted(name)) if name == "web_search"));
    }

    #[test]
    fn parses_successful_result() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"hi there","session_id":"abc"}"#;
        let Some(Event::Finished {
            session_id,
            outcome,
        }) = parse_line(line)
        else {
            panic!("expected a finished event");
        };
        assert_eq!(session_id.as_deref(), Some("abc"));
        assert_eq!(outcome, Ok("hi there".to_string()));
    }

    #[test]
    fn parses_failed_result() {
        let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"session_id":"abc"}"#;
        assert!(matches!(
            parse_line(line),
            Some(Event::Finished { outcome: Err(reason), .. }) if reason == "error_during_execution"
        ));
    }

    #[test]
    fn ignores_other_lines() {
        assert!(parse_line(r#"{"type":"system","subtype":"init"}"#).is_none());
        assert!(parse_line("not json").is_none());
    }
}
