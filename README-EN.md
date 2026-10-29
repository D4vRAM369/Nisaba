<div align="center">

<img src="banner.jpg" alt="NISABA: the Sumerian goddess of writing inscribes a clay tablet among ears of barley, in gold on lapis lazuli" width="100%">

**The local vault for your conversations with AIs.**
Find what you discussed with Claude Code or Codex by word or phrase, and reread it without the noise.

[Español](README.md) · [GPL-3.0](LICENSE) license · v0.1.1

> ⚠️ **Early version (0.x).** It works and is used daily, but it is not stable yet: commands and formats may change.

![Nisaba: search on the left, "conversation only" reader on the right](assets/captura-lector.png)

</div>

> The interface is in Spanish for now; English UI is planned for v0.7.

## Why this project?

I work with several AIs every day. What I learned and decided with them ends up scattered across different formats and folders, and part of it gets lost: **Claude Code deletes sessions older than 30 days by default**. I wanted a place on my own PC where I could get to my project's conversations with the AIs I use most, without depending on their official websites.

Nisaba reads the sessions those tools already keep on your disk, indexes them and lets you **search and reread them nicely formatted**: your messages, the answers, code and Markdown, without walls of tool calls. If a source deletes a session, the tablet keeps it: **the tablet only grows**.

The name comes from Nisaba, the Sumerian goddess of writing and scribes. The vault is called "la tablilla" (the tablet).

## What it does today (v0.1.1)

- **Sources:** Claude Code (`~/.claude/projects`) and Codex (`~/.codex`).
- **Search** by word, quoted phrase or prefix (`medica*`), with source, project and date filters.
- **"Conversation only" reader:** tools collapse into one line and reasoning stays folded. "Ver todo" (show all) displays every event.
- **Health panel:** files seen vs. parsed per source, so nothing goes missing without you knowing.
- **Export** a whole conversation to `.md` or `.pdf`, with a warning if it seems to contain a key or token.
- **Back to the source:** copy the command to resume the session in Claude Code or Codex, or open the project folder.
- **Don't index:** exclude folders before they are read; what was already stored is deleted only if you confirm.
- **Three doors:** the window, the terminal (`nisaba`) and your AIs via **MCP**.
- **No network:** nothing leaves your computer.

## Installation

Download the file for your system from [Releases](https://github.com/D4vRAM369/Nisaba/releases). Every package includes the window (`nisaba-gui`) **and** the terminal tool (`nisaba`).

### Linux

| Format | Install |
|---|---|
| `.deb` (Debian, Ubuntu, Mint…) | `sudo apt install ./Nisaba_0.1.1_amd64.deb` |
| `.rpm` (Fedora, openSUSE…) | `sudo dnf install ./Nisaba-0.1.1-1.x86_64.rpm` |
| AppImage (portable, no install) | `chmod +x Nisaba_0.1.1_amd64.AppImage && ./Nisaba_0.1.1_amd64.AppImage` |
| Flatpak | `flatpak install --user ./Nisaba_0.1.1_x86_64.flatpak` (needs the `org.gnome.Platform//50` runtime from Flathub) |

The Flatpak can only **read** `~/.claude` and `~/.codex`, and has no network access.

### Windows 10/11

| Format | Install |
|---|---|
| Installer (`Nisaba_0.1.1_x64-setup.exe`) | Double-click. Adds `nisaba` to your user `Path`. |
| `.msi` | Double-click (meant for managed deployments). |
| Portable (`Nisaba_0.1.1_x64_portable.zip`) | Unzip and open `nisaba-gui.exe`. The index is stored in `datos\`, next to the `.exe`. |

The binaries are not signed yet, so Windows will show **"Windows protected your PC"**. Click **More info → Run anyway**.

## How to use it

### In the window

1. Open **Nisaba**. The first time it indexes your sessions (about 30 s for ~700 conversations); you can already search meanwhile.
2. Type in the search box: it searches as you type. Use `"an exact phrase"` or a prefix like `medica*`.
3. Narrow down with **Fuente** (source), **Proyecto** (project) and **Fecha** (date), or tick **Razonamiento** (reasoning) / **Código y herramientas** (code and tools) to search there too.
4. Click a result: the reader jumps to the match. **Ver todo** shows every tool call and event.
5. In the reader header: **⤓ .md** saves the whole conversation as Markdown and **⤓ .pdf** opens the print dialog (choose "Save as PDF"). They export what you see: with **Ver todo**, tools and reasoning too. If the conversation seems to contain a key or token, Nisaba warns you first.
6. **▶ Continuar** (continue) copies the command to resume the session (`claude --resume …` from its folder, or `codex resume …`); **📁 Carpeta** (folder) opens it and **⧉ Ruta** (path) copies it.
7. **Ajustes → No indexar** (settings → don't index): add folders (or pick a known project) whose conversations you don't want in the tablet. If something was already stored, it asks whether to delete it; if not, it stays hidden everywhere.
8. **Salud** (health, top right) tells you whether each source is complete.

### In the terminal

```bash
nisaba search "phrase or word"
nisaba search "medica*" --source codex --project Mirlo --since 2026-10-01 --limit 20
nisaba search "something" --json       # output for scripts
nisaba show cx-xxxxxxxx                 # the id is printed by `search`
nisaba show cx-xxxxxxxx --all           # with tools, reasoning and hidden events
nisaba open cx-xxxxxxxx                 # opens that conversation in the window
nisaba export cx-xxxxxxxx               # saves it whole as Markdown (--all, --output <path>)
nisaba exclude ~/Projects/Private       # don't index that folder (--delete removes what was stored)
nisaba exclude --list                   # excluded folders (--remove <folder> to index it again)
nisaba health                           # health of each source
nisaba usage                            # searches per day and channel
```

If `export` finds possible secrets, it shows them masked and writes nothing until you add `--force`. The Spanish commands work too (`buscar`, `ver`, `abrir`, `exportar`, `excluir`, `indexar`, `salud`, `uso`); `nisaba --help` lists both. Messages are still in Spanish.

With Flatpak, prefix `flatpak run --command=nisaba io.github.d4vram369.Nisaba` (for example, `flatpak run --command=nisaba io.github.d4vram369.Nisaba search "something"`).

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
- The usage counter (`nisaba usage`) lives in your index and never leaves your computer.
- What you exclude with **No indexar** is never read or copied; if you keep what was stored, neither search nor MCP shows it.

## Roadmap

| Version | Content |
|---|---|
| v0.1 | Claude Code + Codex, CLI, MCP, window, health panel |
| **v0.1.1** | Export to `.md` and `.pdf` with a secrets warning, back to the original session and folder, "don't index", commands in English too |
| v0.2 | Official Claude.ai and ChatGPT exports: an import pipeline for the ZIP you request and download, so you can browse your whole archive (conversations, projects and attachments) in Nisaba, encrypted at rest with AES-256-GCM and a key derived from your password with Argon2id |
| v0.3 | OpenCode |
| v0.4 | Pi |
| v0.5 | Gemini CLI |
| v0.6 | Antigravity (partial) |
| v0.7 | "Arcilla" light theme, sidebar, global shortcut, English UI |
| v1.0 | All of the above, stable |

## Build from source

```bash
cargo test --workspace
npm --prefix crates/nisaba-gui/ui ci
npm --prefix crates/nisaba-gui/ui run tauri -- dev
```

## License

[GPL-3.0-or-later](LICENSE) © 2026 D4vRAM369.
