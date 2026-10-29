<div align="center">

<img src="banner.jpg" alt="NISABA: the Sumerian goddess of writing inscribes a clay tablet among ears of barley, in gold on lapis lazuli" width="100%">

**The local vault for your conversations with AIs.**
Find what you discussed with Claude Code or Codex by word or phrase, and reread it without the noise.

[Español](README.md) · [GPL-3.0](LICENSE) license · v0.1.0

> ⚠️ **Prerelease.** It works and is used daily, but it is not stable yet: commands and formats may change.

![Nisaba: search on the left, "conversation only" reader on the right](assets/captura-lector.png)

</div>

> The interface is in Spanish for now; English UI is planned for v0.7.

## Why this project?

I work with several AIs every day. What I learned and decided with them ends up scattered across different formats and folders, and part of it gets lost: **Claude Code deletes sessions older than 30 days by default**. When I want to get back to "that conversation where we solved X", I end up running `grep` over unreadable JSON files, or I give up.

Nisaba reads the sessions those tools already keep on your disk, indexes them and lets you **search and reread them nicely formatted**: your messages, the answers, code and Markdown, without walls of tool calls. If a source deletes a session, the tablet keeps it: **the tablet only grows**.

The name comes from Nisaba, the Sumerian goddess of writing and scribes. The vault is called "la tablilla" (the tablet).

## What it does today (v0.1)

- **Sources:** Claude Code (`~/.claude/projects`) and Codex (`~/.codex`).
- **Search** by word, quoted phrase or prefix (`medica*`), with source, project and date filters.
- **"Conversation only" reader:** tools collapse into one line and reasoning stays folded. "Ver todo" (show all) displays every event.
- **Health panel:** files seen vs. parsed per source, so nothing goes missing without you knowing.
- **Three doors:** the window, the terminal (`nisaba`) and your AIs via **MCP**.
- **No network:** nothing leaves your computer.

## Installation

Download the file for your system from [Releases](https://github.com/D4vRAM369/Nisaba/releases). Every package includes the window (`nisaba-gui`) **and** the terminal tool (`nisaba`).

### Linux

| Format | Install |
|---|---|
| `.deb` (Debian, Ubuntu, Mint…) | `sudo apt install ./Nisaba_0.1.0_amd64.deb` |
| `.rpm` (Fedora, openSUSE…) | `sudo dnf install ./Nisaba-0.1.0-1.x86_64.rpm` |
| AppImage (portable, no install) | `chmod +x Nisaba_0.1.0_amd64.AppImage && ./Nisaba_0.1.0_amd64.AppImage` |
| Flatpak | `flatpak install --user ./Nisaba_0.1.0_x86_64.flatpak` (needs the `org.gnome.Platform//50` runtime from Flathub) |

The Flatpak can only **read** `~/.claude` and `~/.codex`, and has no network access.

### Windows 10/11

| Format | Install |
|---|---|
| Installer (`Nisaba_0.1.0_x64-setup.exe`) | Double-click. Adds `nisaba` to your user `Path`. |
| `.msi` | Double-click (meant for managed deployments). |
| Portable (`Nisaba_0.1.0_x64_portable.zip`) | Unzip and open `nisaba-gui.exe`. The index is stored in `datos\`, next to the `.exe`. |

The binaries are not signed yet, so Windows will show **"Windows protected your PC"**. Click **More info → Run anyway**.

## How to use it

### In the window

1. Open **Nisaba**. The first time it indexes your sessions (about 30 s for ~700 conversations); you can already search meanwhile.
2. Type in the search box: it searches as you type. Use `"an exact phrase"` or a prefix like `medica*`.
3. Narrow down with **Fuente** (source), **Proyecto** (project) and **Fecha** (date), or tick **Razonamiento** (reasoning) / **Código y herramientas** (code and tools) to search there too.
4. Click a result: the reader jumps to the match. **Ver todo** shows every tool call and event.
5. **Salud** (health, top right) tells you whether each source is complete.

### In the terminal

```bash
nisaba buscar "phrase or word"
nisaba buscar "medica*" --fuente codex --proyecto Mirlo --desde 2026-10-01 --limite 20
nisaba buscar "something" --json       # output for scripts
nisaba ver cx-xxxxxxxx                  # the id is printed by `buscar`
nisaba ver cx-xxxxxxxx --todo           # with tools, reasoning and hidden events
nisaba abrir cx-xxxxxxxx                # opens that conversation in the window
nisaba salud                            # health of each source
nisaba uso                              # searches per day and channel
```

With Flatpak, prefix `flatpak run --command=nisaba io.github.d4vram369.Nisaba` (for example, `flatpak run --command=nisaba io.github.d4vram369.Nisaba buscar "something"`).

### From your AI (MCP)

Nisaba ships an MCP server that only talks over `stdio` (no ports opened). To let Claude Code search your history:

```bash
claude mcp add --scope user nisaba -- nisaba mcp
```

With Flatpak:

```bash
claude mcp add --scope user nisaba -- flatpak run --command=nisaba io.github.d4vram369.Nisaba mcp
```

Then, in a new session, ask something like *"search my conversations for where we decided to use FTS5"*.

### Folders and variables

| What | Default | Variable to change it |
|---|---|---|
| Index (the tablet) | Linux: `~/.local/share/nisaba` · Windows: `%APPDATA%\nisaba` · Portable: `datos\` next to the `.exe` | `NISABA_HOME` |
| Claude Code sessions | `~/.claude/projects` | `NISABA_CLAUDE_DIR` |
| Codex sessions | `~/.codex` | `NISABA_CODEX_DIR` |
| Claude Code settings (cleanup warning) | `~/.claude/settings.json` | `NISABA_CLAUDE_SETTINGS` |

## Privacy

- **No network, no accounts, no telemetry.** Not even fonts are downloaded.
- Nisaba **only reads** the Claude Code and Codex folders; it never writes to them.
- The usage counter (`nisaba uso`) lives in your index and never leaves your computer.

## Roadmap

| Version | Content |
|---|---|
| **v0.1** | Claude Code + Codex, CLI, MCP, window, health panel |
| v0.2 | OpenCode |
| v0.3 | Pi |
| v0.4 | Gemini CLI |
| v0.5 | Antigravity (partial) |
| v0.6 | ChatGPT and Claude.ai ZIP exports |
| v0.7 | "Arcilla" light theme, sidebar, global shortcut, English UI, secrets warning, Markdown export |
| v1.0 | All of the above, stable |

## Build from source

```bash
cargo test --workspace
npm --prefix crates/nisaba-gui/ui ci
npm --prefix crates/nisaba-gui/ui run tauri -- dev
```

## License

[GPL-3.0-or-later](LICENSE) © 2026 D4vRAM369.
