# Architecture

## Lifecycle

1. Launch: activation policy set to Accessory (no Dock icon or menu bar),
   settings loaded, icons registered (`icon::Assets`), the window opened as
   `WindowKind::PopUp` (440 wide, no title bar, blurred background),
   the native `Panel` configured, and the configured shortcut registered.
2. The shortcut calls `Companion::toggle`:
   - **visible and key** → `panel::hide` (`orderOut:`).
   - otherwise → apply the expiry rule, **start `claude` if it isn't
     running** (prewarm), `panel::show` (`makeKeyAndOrderFront:`), focus,
     scroll to the bottom.

   `Panel::configure` switches the panel to a borderless non-activating
   style mask, which removes the title bar, traffic lights and frame
   outline, and clips the content view and gpui's blur subview to
   `CORNER_RADIUS`. The root element draws the matching hairline border. The
   window uses `WindowBackgroundAppearance::Blurred`, and the theme colours
   are partly transparent so the blur shows through.

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
5. ⌘N or **+** starts a new chat. ⌘Q quits.

## Layout (`companion.rs`)

- **Compact** (no conversation, chat view): the panel is just the composer,
  `COMPACT_HEIGHT` tall.
- **Expanded** (any message, or settings open): transcript above, composer
  as an inset card below, `EXPANDED_HEIGHT` tall.
- `sync_height` runs on every notify (`observe_self`) and resizes only on
  change, via `cx.defer` with the stored `Panel`. Two things matter here:
  - **Before the frame:** the resize lands before the next draw, so a view
    change and its new size appear together. Resizing from `render` drew one
    frame at the old size, which cut off the composer's icon row when
    shrinking.
  - **Outside window updates:** AppKit reports the resize synchronously, and
    gpui drops that report if it arrives during an update of the same window
    (`defer_in`), leaving gpui drawing at the old size.

  `Panel::set_height` keeps the top edge fixed, since AppKit frames grow
  from the bottom-left.
- **Focus:** in the chat view the root's mouse-down re-focuses the input and
  calls `prevent_default`, so clicking anywhere never takes keystrokes away
  from it. `Panel::show` also makes gpui's view first responder, because
  changing the style mask can leave the window itself as first responder,
  and then keystrokes never reach gpui.
- Composer row: **+** new chat, **globe** toggles web search and page
  reading together (blue when on; the same settings as the Settings page,
  restarting `claude` once idle), **sliders** open Settings, and the
  **send** button (light when there's text to send).
- Under each reply: **Copy** (raw Markdown; shows a tick for 1.5s),
  **Read aloud** (`/usr/bin/say -f -` fed the document's
  `plain_text()`, code blocks skipped; click again to stop, as dropping the
  task kills `say`) and, on the latest reply when idle, **Retry**.
  Retry drops the reply and re-sends the question. The CLI session can't be
  rewound, so Claude still has the earlier answer in context.
- **Moving the panel:** press anywhere and drag. After `DRAG_THRESHOLD`
  (4px) of movement with the left button held, `panel::begin_drag` hands the
  current event to `performWindowDragWithEvent:` and the window server moves
  the window until release. Waiting for movement keeps plain clicks working
  on buttons and links. Drags are skipped while the text input
  `is_selecting()`, so drags there select text. (Don't stop propagation in
  the input's mouse-down: gpui runs an element's listeners in reverse, so
  that also cancels the input's own click-to-focus.) gpui's `start_window_move` is a no-op
  on macOS, hence the native call.
- Icons are hand-drawn 24×24 stroke SVGs in `assets/icons/`, compiled in by
  the `icons!` macro and rendered with `svg()` (tinted by text colour).

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
