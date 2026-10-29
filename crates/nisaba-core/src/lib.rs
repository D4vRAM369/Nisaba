pub mod adaptadores;
pub mod db;
pub mod indexador;
pub mod modelo;
pub mod titulo;

use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn entorno(nombre: &str) -> Option<PathBuf> {
    std::env::var_os(nombre)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Carpeta de la bóveda: `NISABA_HOME`, o `$XDG_DATA_HOME/nisaba`, o `~/.local/share/nisaba`.
pub fn nisaba_home() -> PathBuf {
    if let Some(p) = entorno("NISABA_HOME") {
        return p;
    }
    entorno("XDG_DATA_HOME")
        .unwrap_or_else(|| home().join(".local/share"))
        .join("nisaba")
}

/// Carpeta de proyectos de Claude Code: `NISABA_CLAUDE_DIR` o `~/.claude/projects`.
pub fn dir_claude() -> PathBuf {
    entorno("NISABA_CLAUDE_DIR").unwrap_or_else(|| home().join(".claude/projects"))
}

/// Carpeta de Codex: `NISABA_CODEX_DIR` o `~/.codex`.
pub fn dir_codex() -> PathBuf {
    entorno("NISABA_CODEX_DIR").unwrap_or_else(|| home().join(".codex"))
}
