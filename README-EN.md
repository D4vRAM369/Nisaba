<div align="center">

<img src="banner.jpg" alt="NISABA: the Sumerian goddess of writing inscribes a clay tablet among ears of barley, in gold on lapis lazuli" width="100%">

**The local vault for your conversations with AIs.**
Find what you discussed with Claude Code, Codex or Claude.ai by word or phrase, and reread it without the noise.

[Español](README.md) · [GPL-3.0](LICENSE) license · v0.1.2

> ⚠️ **Early version (0.x).** It works and is used daily, but it is not stable yet: commands and formats may change.

![Nisaba: search on the left, "conversation only" reader on the right](assets/captura-lector.png)

</div>

> The interface is in Spanish for now; English UI is planned for v0.7.

## Why this project?

I work with several AIs every day. What I learned and decided with them ends up scattered across different formats and folders, and part of it gets lost: **Claude Code deletes sessions older than 30 days by default**. I wanted a place on my own PC where I could get to my project's conversations with the AIs I use most, without depending on their official websites.

Nisaba reads the sessions those tools already keep on your disk (and the ZIP exports you download from the web), indexes them and lets you **search and reread them nicely formatted**: your messages, the answers, code and Markdown, without walls of tool calls. If a source deletes a session, the tablet keeps it: **the tablet only grows**.

The name comes from Nisaba, the Sumerian goddess of writing and scribes. The vault is called "la tablilla" (the tablet).

## What it does today (v0.1.2)

- **Local sources:** Claude Code (`~/.claude/projects`) and Codex (`~/.codex`).
- **Encrypted web archive (Claude.ai):** imports the folder of ZIPs or a single `.zip` exported from Claude.ai (conversations, projects and their documents, memories, HTML *frames* and design chats; `light_metadata` is never read) into a separate database encrypted with **SQLCipher** (`tablilla-web.db`), without extracting temporary files to disk.
- **Search** by word, quoted phrase or prefix (`medica*`), with source, project and date filters. When the web archive is unlocked, local and web results are merged seamlessly.
- **"Conversation only" reader + Details panel:** tools collapse into one line and reasoning stays folded. On Claude.ai conversations (`cw-…`), a side panel shows the project and its documents, attachments (extracted text), cited sources, tools, edited branches, previous versions, and a preview of HTML *frames* inside an empty-`sandbox` `iframe`.
- **Health panel:** files seen vs. parsed per source, so nothing goes missing without you knowing.
- **Export** a whole conversation to `.md` or `.pdf`, with a warning if it seems to contain a key or token (and a notice that exported files remain unencrypted).
- **Back to the source:** copy the command to resume the local session in Claude Code or Codex, or open the project folder.
- **Don't index:** exclude folders before they are read; what was already stored is deleted only if you confirm.
- **Three doors:** the window, the terminal (`nisaba`) and your AIs via **MCP** (the MCP server only reads the local tablet and never opens the encrypted web archive).
- **No network:** nothing leaves your computer.

## Installation

Download the file for your system from [Releases](https://github.com/D4vRAM369/Nisaba/releases). Every package includes the window (`nisaba-gui`) **and** the terminal tool (`nisaba`).

### Linux

| Format | Install |
|---|---|
| `.deb` (Debian, Ubuntu, Mint…) | `sudo apt install ./Nisaba_0.1.2_amd64.deb` |
| `.rpm` (Fedora, openSUSE…) | `sudo dnf install ./Nisaba-0.1.2-1.x86_64.rpm` |
| AppImage (portable, no install) | `chmod +x Nisaba_0.1.2_amd64.AppImage && ./Nisaba_0.1.2_amd64.AppImage` |
| Flatpak | `flatpak install --user ./Nisaba_0.1.2_x86_64.flatpak` (needs the `org.gnome.Platform//50` runtime from Flathub) |

The Flatpak can only **read** `~/.claude` and `~/.codex`, and has no network access.

### Windows 10/11

| Format | Install |
|---|---|
| Installer (`Nisaba_0.1.2_x64-setup.exe`) | Double-click. Adds `nisaba` to your user `Path`. |
| `.msi` | Double-click (meant for managed deployments). |
| Portable (`Nisaba_0.1.2_x64_portable.zip`) | Unzip and open `nisaba-gui.exe`. The index is stored in `datos\`, next to the `.exe`. |

The binaries are not signed yet, so Windows will show **"Windows protected your PC"**. Click **More info → Run anyway**.

## How to use it

### In the window

1. Open **Nisaba**. The first time it indexes your sessions (about 30 s for ~700 conversations); you can already search meanwhile.
2. Type in the search box: it searches as you type. Use `"an exact phrase"` or a prefix like `medica*`.
3. Narrow down with **Fuente** (source), **Proyecto** (project) and **Fecha** (date), or tick **Razonamiento** (reasoning) / **Código y herramientas** (code and tools) to search there too.
4. Click a result: the reader jumps to the match. **Ver todo** shows every tool call and event.
5. **Archivo web (web archive, top right):**
   - The first time, choose a password (at least 12 characters), save the **128-bit recovery code**, and tick the confirmation box.
   - Once unlocked, click **📁 Elegir carpeta de exportación…** (for example, the folder holding your Claude.ai export ZIPs) or **📦 Elegir archivo .zip…**, or drag & drop it onto the window.
   - When opening a Claude.ai conversation (`cw-…`), the **Detalles** side panel shows its project documents, extracted attachment text (Claude.ai exports do not include original binary files), citations, tools, branches and previous versions.
   - Closing the window locks the web archive automatically.
6. In the reader header: **⤓ .md** saves the whole conversation as Markdown and **⤓ .pdf** opens the print dialog (choose "Save as PDF"). They export what you see: with **Ver todo**, tools and reasoning too. If the conversation seems to contain a key or token, Nisaba warns you first.
7. **▶ Continuar** (continue) copies the command to resume the session (`claude --resume …` from its folder, or `codex resume …`); **📁 Carpeta** (folder) opens it and **⧉ Ruta** (path) copies it.
8. **Ajustes → No indexar** (settings → don't index): add folders (or pick a known project) whose conversations you don't want in the tablet. If something was already stored, it asks whether to delete it; if not, it stays hidden everywhere.
9. **Salud** (health, top right) tells you whether each local source is complete.

### In the terminal

```bash
nisaba search "phrase or word"
nisaba search "medica*" --source codex --project Mirlo --since 2026-10-01 --limit 20
nisaba search "something" --json       # output for scripts
nisaba show cx-xxxxxxxx                 # the id is printed by `search`
nisaba show cx-xxxxxxxx --all           # with tools, reasoning and hidden events
nisaba open cx-xxxxxxxx                 # opens that conversation in the window
nisaba export cx-xxxxxxxx               # saves it whole as Markdown (--all, --output <path>, --overwrite)
nisaba exclude ~/Projects/Private       # don't index that folder (--delete removes what was stored)
nisaba exclude --list                   # excluded folders (--remove <folder> to index it again)
nisaba health                           # health of each source
nisaba usage                            # searches per day and channel

# Encrypted web archive (Claude.ai)
nisaba import ~/Downloads/Claude        # imports a folder of ZIPs or a single .zip
nisaba search "phrase" --web            # includes the web archive in search results
nisaba show cw-xxxxxxxx --version 1     # reads a web conversation or a previous version
nisaba password                         # changes the password without re-encrypting the DB
nisaba recover                          # resets the password using the recovery code
```

If `export` finds possible secrets, it shows them masked and writes nothing until you add `--force`. It won't replace an existing file either: use `--overwrite` for that. The Spanish commands work too (`buscar`, `ver`, `abrir`, `exportar`, `excluir`, `indexar`, `salud`, `uso`, `importar`, `contrasena`, `recuperar`); `nisaba --help` lists both. Messages are still in Spanish.

With Flatpak, prefix `flatpak run --command=nisaba io.github.d4vram369.Nisaba` (for example, `flatpak run --command=nisaba io.github.d4vram369.Nisaba search "something"`).

### From your AI (MCP)

Nisaba ships an MCP server that only talks over `stdio` (no ports opened). For privacy, the MCP server only queries the local tablet (`tablilla.db`) and never touches the encrypted web archive. To let Claude Code search your local history:

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
| Index (`tablilla.db`, `tablilla-web.db`, `boveda.json`) | Linux: `~/.local/share/nisaba` · Windows: `%APPDATA%\nisaba` · Portable: `datos\` next to the `.exe` | `NISABA_HOME` |
| Claude Code sessions | `~/.claude/projects` | `NISABA_CLAUDE_DIR` |
| Codex sessions | `~/.codex` | `NISABA_CODEX_DIR` |
| Claude Code settings (cleanup warning) | `~/.claude/settings.json` | `NISABA_CLAUDE_SETTINGS` |

## Privacy and web archive encryption

- **No network, no accounts, no telemetry.** Not even fonts are downloaded.
- Nisaba **only reads** the Claude Code and Codex folders and the ZIPs you import; it never modifies or deletes them.
- **How the web archive is encrypted:** imported conversations and their FTS5 index live in `tablilla-web.db`, encrypted page by page with **SQLCipher (AES-256-CBC + HMAC-SHA512)**. The random 256-bit master key is stored wrapped twice in `boveda.json` using **AES-256-GCM** with keys derived via **Argon2id** (once with your password and once with your 128-bit recovery code).
- **Backups:** always back up **`tablilla-web.db` and `boveda.json` together**. One without the other cannot be opened.
- **What it protects and what it doesn't:** it protects your web conversations on disk and in backups while the archive is locked. It does not encrypt the original ZIPs you downloaded from your browser (move them to encrypted storage or delete them after importing), `.md`/`.pdf` exports are saved unencrypted, and encryption at rest cannot protect against malware running in your user session while the archive is unlocked.
- The usage counter (`nisaba usage`) lives in your local index and never leaves your computer.
- What you exclude with **No indexar** is never read or copied; if you keep what was stored, neither search nor MCP shows it.

## Roadmap

| Version | Content |
|---|---|
| v0.1 | Claude Code + Codex, CLI, MCP, window, health panel |
| v0.1.1 | Export to `.md` and `.pdf` with a secrets warning, back to the original session and folder, "don't index", commands in English too |
| **v0.1.2** | Encrypted web archive (SQLCipher + Argon2id/AES-256-GCM with recovery code) and official Claude.ai export importer (conversations, projects, memories, HTML *frames* and design chats) with Details panel |
| v0.2 | Official ChatGPT export importer on top of the encrypted web archive |
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
