# Architecture

## Lifecycle

1. Launch: activation policy set to Accessory (no Dock icon or menu bar),
   key bindings registered, the window opened as `WindowKind::PopUp`
   (480×560, transparent titlebar, not resizable), and ⌥Space registered.
2. ⌥Space calls `Companion::toggle`:
   - **visible and active** → `cx.hide()` (hides the whole app).
   - otherwise → apply the expiry rule, `cx.activate(true)`,
     `window.activate_window()`, focus the input, scroll to the bottom.
3. Esc hides the window. So does clicking away (window deactivation), as with
   Spotlight.
4. ⌘N starts a new chat: kills any running reply and clears state. ⌘Q quits.

## Session expiry (`state.rs`)

`state.json` in `~/Library/Application Support/claude-companion/`:

```json
{ "session_id": "…", "last_activity": 1793500000, "transcript": [{ "role": "user", "text": "…" }] }
```

- On every show (and while no reply is pending), if `now - last_activity ≥ 5 min`,
  the whole state resets. A transcript with no `last_activity` (the app quit
  before the first reply) also counts as stale.
- `last_activity` is set only when a reply finishes successfully, so a long
  answer doesn't count against the timeout.
- Writes go to `state.json.tmp` and are then renamed over the original.

## CLI driver (`claude.rs`)

```
claude -p --output-format stream-json --verbose --include-partial-messages \
  --tools WebSearch,WebFetch --allowedTools WebSearch,WebFetch \
  --strict-mcp-config [--resume <session_id>]
```

The prompt is written to stdin. stdout and stderr are read together
(`smol::future::zip`) so neither pipe can fill up and block.

Events used (checked against CLI 2.1.316):

| stream-json line | `Event` |
| --- | --- |
| `stream_event` / `message_start` | `MessageStarted`: adds a paragraph break between assistant turns (e.g. around a search) |
| `stream_event` / `content_block_delta` with `text_delta` | `TextDelta` |
| `stream_event` / `content_block_start` with `tool_use` or `server_tool_use` | `ToolStarted`: shows a status line ("Searching the web…") |
| `result` | `Finished { session_id, outcome }`: failed if `is_error` or `subtype != "success"` |

If the process exits without a `result`, its stderr (or exit status) is shown
as an error message in the transcript.

The streamed text is preferred over `result.result`, because `result` only
holds the final assistant message and would drop any text written before a
tool call.

## Text input (`text_input.rs`)

A single-line field from gpui's `input.rs` example. It implements
`EntityInputHandler`, so IME, the character palette and selection work.
Additions: a `Submit` event on Enter (ignored while IME text is marked),
`take()`, ⌘←/⌘→, and horizontal scrolling (`scroll_x`) applied consistently
across painting, mouse hit-testing and IME bounds.

## Known limitations

- Replies are shown as plain text: no Markdown rendering, and no selecting or
  copying text in the transcript.
- The input is single-line, and pasted newlines become spaces.
- If a `--resume` fails (e.g. the session file was deleted), every following
  message fails too until you press ⌘N.
- The hotkey is hard-coded to ⌥Space.
