# Architecture

## Lifecycle

1. Launch: activation policy set to Accessory (no Dock icon or menu bar),
   settings loaded, key bindings registered, the window opened as
   `WindowKind::PopUp` (480×560, transparent titlebar, not resizable), and
   the configured shortcut registered.
2. The shortcut calls `Companion::toggle`:
   - **visible and key** → `panel::hide` (`orderOut:`).
   - otherwise → apply the expiry rule, **start `claude` if it isn't
     running** (prewarm), `panel::show` (`makeKeyAndOrderFront:`), focus,
     scroll to the bottom.

   `panel::show` also hides the traffic lights on every show. The window
   uses `WindowBackgroundAppearance::Blurred`, and the theme colours are
   partly transparent to let it show through.

   The PopUp window is a non-activating `NSPanel`, so it takes keyboard
   focus while the previous app stays frontmost, and hiding returns focus to
   that app. The app itself is never activated or hidden: doing that
   (`cx.activate` / `cx.hide`) raced with gpui's key-window workaround and
   made the shortcut unreliable.
3. Esc hides the window (or leaves the settings view). With "Hide when
   clicking away" on, losing key status hides it, but only if focus is still
   elsewhere 150ms later (`BLUR_GRACE`), since key status can flicker while
   the panel is being shown.
4. On hide, an idle timer is armed. If the window is still hidden and no
   reply is running when it fires (after the session timeout), the `claude`
   process is killed.
5. ⌘N starts a new chat. The header button switches between chat and settings. ⌘Q quits.

## Persistence (`storage.rs`)

Both files live in `~/Library/Application Support/claude-companion/` and are
written to `*.json.tmp` and then renamed over the original.

`state.json`, the conversation:

```json
{ "session_id": "…", "last_activity": 1793500000, "transcript": [{ "role": "user", "text": "…" }] }
```

`settings.json`: see [Settings](#settings). Fields missing from the file take
their default values (`#[serde(default)]`); a file that can't be parsed
falls back to all defaults.

## Session expiry (`state.rs`)

- On every show, unless a reply is running, the state resets if
  `now - last_activity ≥ session timeout`. A transcript with no
  `last_activity` (the app quit before the first reply) also counts as stale.
- `last_activity` is set when a reply's final message ends, so a long answer
  doesn't count against the timeout.

## CLI driver (`claude.rs`)

One long-lived process per conversation:

```
claude -p --verbose --input-format stream-json --output-format stream-json \
  --include-partial-messages --tools <enabled> [--allowedTools <enabled>] \
  --strict-mcp-config [--model <m>] [--effort <e>] [--resume <session_id>]
```

Each message is written to stdin as a single line:
`{"type":"user","message":{"role":"user","content":"…"}}`. The process
stays alive between turns. `std::process` is used for spawning, with stdout
and stderr wrapped in `smol::Unblock`. stdin is written synchronously, since
messages are tiny. Dropping `Process` kills and reaps the child.

Measured on CLI 2.1.316 (Sonnet 5.5, low effort):

| | Cold `claude -p` per message | Warm process |
| --- | --- | --- |
| Startup before the model call | ~1.0s | paid during prewarm |
| Send → first text | ~3.2s (Opus, default effort) | ~2.0s |
| Reply end → `result` (`post_turn_summary`) | ~1.8s | ~1.8s, but the input is already unlocked |

`post_turn_summary` is internal CLI work with no documented flag to turn it
off (`CLAUDE_CODE_POST_TURN_MEMORY=0` made no difference). A message sent
during it is queued and starts once it finishes.

### Events

| stream-json line | `Event` | Effect |
| --- | --- | --- |
| `stream_event` / `message_start` | `MessageStarted` | paragraph break between assistant messages (e.g. around a tool call) |
| `stream_event` / `content_block_delta` with `text_delta` | `TextDelta` | appended to the pending reply |
| `stream_event` / `content_block_start` with `tool_use` or `server_tool_use` | `ToolStarted` | status line ("Searching the web…") |
| `stream_event` / `message_delta` with a stop reason | `MessageEnded { last }` | `last` unless `tool_use` / `pause_turn`. On the last message: commit, unlock, mark unsettled |
| `result` | `Finished { session_id, outcome }` | saves `session_id`; on error, shows it. Ignored if no turn is running |

### Turn states (`companion.rs`)

```
idle ──send──▶ pending ──last MessageEnded──▶ unsettled ──result──▶ idle
                  └──────────── result (error before end) ────────────┘
```

`is_busy()` = pending or unsettled. It blocks expiry, idle shutdown and
restarts for settings changes. Sending is only blocked while pending.

If the process exits on its own:
- **while busy:** an error is shown and the session is dropped; the next
  message starts a new process with `--resume`.
- **while idle, with stderr output** (e.g. `No conversation found with
  session ID`): `session_id` is cleared and a notice says the next message
  starts a new conversation.

## Settings

Opened with the header's Settings button (Esc or Back returns). Each setting is a `Segmented<T: Choice>` control. The panel
edits a copy and emits `SettingsEvent::Changed`; `Companion` saves and
applies it.

| Setting | Values (default **bold**) | Applies |
| --- | --- | --- |
| Model | Default (CLI's), Fable, Opus, **Sonnet**, Haiku | restarts `claude` with `--resume` once idle |
| Effort | Default, **Low**, Medium, High, XHigh, Max | same |
| Search the web / Read web pages | **On** / Off | same; both off passes `--tools ""` |
| Start a new chat after idle | 1, **5**, 10, 30 min, 1 hour | next show / idle timer |
| Shortcut | **⌥Space**, ⌃Space, ⌥⇧Space, ⌘⇧Space | at once. The new key is registered before the old one is released; on failure the old one stays and the panel shows why |
| Hide when clicking away | **On** / Off | at once |

To add an enum setting, declare it with `choices!` in `settings.rs`, add the
field to `Settings` and its `Default`, and add one `self.setting(…)` line in
`settings_panel.rs`.

## Text input (`text_input.rs`)

A single-line field from gpui's `input.rs` example. It implements
`EntityInputHandler`, so IME, the character palette and selection work.
Additions: a `Submit` event on Enter (ignored while IME text is marked),
`take()`, ⌘←/⌘→, and horizontal scrolling (`scroll_x`) applied consistently
across painting, mouse hit-testing and IME bounds.

## Markdown (`markdown.rs`, `markdown_view.rs`, `highlight.rs`)

Assistant replies render as Markdown; user messages and errors stay plain.

- `markdown::parse` turns pulldown-cmark events (tables, strikethrough, task
  lists and GFM on) into a small model: `Block` = paragraph, heading, code,
  list, quote, table, rule; `Inline` = text plus marks and links. It is
  GPUI-free so it can be unit tested.
- Inline marks can nest (`**bold *both***`), but GPUI highlight runs must
  not overlap, so `InlineBuilder::finish` splits them at every boundary and
  merges the styles.
- Unclosed constructs (a code fence while a reply is still streaming) are
  closed in `Builder::finish`, so partial replies render sensibly.
- Code blocks are coloured by syntect (`base16-ocean.dark`, pure-Rust
  `fancy-regex`). Unknown languages render plain; `grammar_token` maps
  common fence labels the bundled grammars lack (`ts` → `js`, `zsh` → `sh`,
  …). The grammars load on a background thread at launch
  (`highlight::preload`).
- `DocumentCache` (owned by `Companion`) keys parsed documents by a hash of
  their text and drops entries unused in the last render, so a message is
  parsed and highlighted once. The streaming reply is re-parsed only when
  new text arrives.
- Rendering: code blocks have a language label, a Copy button and
  horizontal scrolling (no wrapping, Menlo). Links are clickable
  (`InteractiveText` → `open_url`). Inline code can't change font family
  through a highlight, so it uses colour and a background instead.

## Known limitations

- Text in the transcript can't be selected or copied (code blocks have a
  Copy button). Images in replies show as their alt text.
- The input is single-line, and pasted newlines become spaces.
- Shortcuts come from a fixed list; there is no key recorder.
