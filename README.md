# Claude Companion

A small macOS background app: press **⌥Space** anywhere to open a floating chat
window backed by `claude -p`. Conversations carry on as long as replies keep
coming within 5 minutes of each other; after that, the next time you open it
you start fresh.

The companion can search and read the web (`WebSearch`, `WebFetch`) but has no
other tools and no MCP servers.

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

- [Architecture](docs/architecture.md): lifecycle, session expiry, CLI flags
  and events, known limitations.
