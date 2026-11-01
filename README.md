# Claude Companion

A small macOS background app: press **⌥Space** anywhere to open a floating chat
window backed by a warm `claude -p` process. Conversations carry on as long as
you come back within 5 minutes of the last reply; after that, the next time
you open it you start fresh.

The companion can search and read the web (`WebSearch`, `WebFetch`) but has no
other tools and no MCP servers. It defaults to Sonnet at low effort for quick
replies. Change the model, effort, web tools, idle timeout, shortcut and
click-away behaviour from the sliders icon in the composer. Attach images with
**+**, by pasting, or by dropping them on the panel. Text in the chat can be
selected and copied, and each reply can be copied, read aloud or retried.

## Run

Needs Rust (stable), Xcode, and Claude Code installed and logged in.

```bash
cargo run
```

| Key | Action |
| --- | --- |
| ⌥Space | Show / hide |
| Enter | Send |
| Esc | Hide |
| ⌘N | New chat |
| ⌘Q | Quit |

Set `CLAUDE_COMPANION_CLI` if `claude` isn't in `~/.local/bin`,
`/opt/homebrew/bin` or `/usr/local/bin`.

## Docs

- [Architecture](docs/architecture.md): lifecycle, warm CLI process, turn
  states, settings, timings, known limitations.

## License

[MIT](LICENSE)
