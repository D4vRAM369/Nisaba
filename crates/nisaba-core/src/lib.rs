pub mod adaptadores;
pub mod busqueda;
pub mod db;
pub mod exclusiones;
pub mod exportar;
pub mod indexador;
pub mod lector;
pub mod modelo;
pub mod salud;
pub mod titulo;

use std::path::{Path, PathBuf};

/// Carpeta personal del usuario (`HOME` en Unix, `USERPROFILE` en Windows), si existe.
pub fn dir_personal() -> Option<PathBuf> {
    std::env::home_dir().filter(|p| !p.as_os_str().is_empty())
}

fn entorno(nombre: &str) -> Option<PathBuf> {
    std::env::var_os(nombre)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Decide la carpeta de la bóveda sin tocar el entorno real, para poder probar cada sistema.
fn elegir_home(
    var: &dyn Fn(&str) -> Option<PathBuf>,
    dir_exe: Option<&Path>,
    personal: Option<&Path>,
    windows: bool,
) -> Option<PathBuf> {
    if let Some(p) = var("NISABA_HOME") {
        return Some(p);
    }
    if windows {
        if let Some(d) = dir_exe.filter(|d| d.join("portable").is_file()) {
            return Some(d.join("datos"));
        }
        if let Some(a) = var("APPDATA") {
            return Some(a.join("nisaba"));
        }
    }
    var("XDG_DATA_HOME")
        .or_else(|| personal.map(|h| h.join(".local/share")))
        .map(|d| d.join("nisaba"))
}

fn dir_exe() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

fn home_elegido() -> Option<PathBuf> {
    elegir_home(
        &entorno,
        dir_exe().as_deref(),
        dir_personal().as_deref(),
        cfg!(windows),
    )
}

/// Carpeta de la bóveda: `NISABA_HOME`; en Windows, `datos` junto al `.exe` si hay un archivo
/// `portable`, o `%APPDATA%\nisaba`; si no, `$XDG_DATA_HOME/nisaba` o `~/.local/share/nisaba`.
pub fn nisaba_home() -> PathBuf {
    // ponytail: la ruta relativa nunca se usa porque `comprobar_entorno` corta antes.
    home_elegido().unwrap_or_else(|| PathBuf::from("nisaba"))
}

/// Error claro si faltan la carpeta personal y las variables que la sustituyen.
pub fn comprobar_entorno() -> Result<(), String> {
    validar_entorno(
        home_elegido().is_some(),
        dir_personal().is_some(),
        entorno("NISABA_CLAUDE_DIR").is_some() && entorno("NISABA_CODEX_DIR").is_some(),
    )
}

fn validar_entorno(hay_home: bool, hay_personal: bool, hay_fuentes: bool) -> Result<(), String> {
    if !hay_home || !(hay_personal || hay_fuentes) {
        return Err("No encuentro tu carpeta personal. Define NISABA_HOME, NISABA_CLAUDE_DIR y NISABA_CODEX_DIR.".into());
    }
    Ok(())
}

/// Crea la carpeta de la bóveda con un error que dice qué ruta falló (CLI y GUI).
pub fn preparar_home(ruta: &Path) -> Result<(), String> {
    std::fs::create_dir_all(ruta)
        .map_err(|e| format!("No puedo crear la carpeta de datos {}: {e}", ruta.display()))
}

/// Ajustes de Claude Code: `NISABA_CLAUDE_SETTINGS` o `~/.claude/settings.json`.
pub fn ajustes_claude() -> PathBuf {
    entorno("NISABA_CLAUDE_SETTINGS").unwrap_or_else(|| personal().join(".claude/settings.json"))
}

fn personal() -> PathBuf {
    dir_personal().unwrap_or_default()
}

/// Carpeta de proyectos de Claude Code: `NISABA_CLAUDE_DIR` o `~/.claude/projects`.
pub fn dir_claude() -> PathBuf {
    entorno("NISABA_CLAUDE_DIR").unwrap_or_else(|| personal().join(".claude/projects"))
}

/// Carpeta de Codex: `NISABA_CODEX_DIR` o `~/.codex`.
pub fn dir_codex() -> PathBuf {
    entorno("NISABA_CODEX_DIR").unwrap_or_else(|| personal().join(".codex"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

    fn vars(pares: &[(&str, &str)]) -> impl Fn(&str) -> Option<PathBuf> {
        let m: HashMap<String, PathBuf> = pares
            .iter()
            .map(|(k, v)| (k.to_string(), PathBuf::from(v)))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn nisaba_home_manda() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("portable"), "").unwrap();
        let r = elegir_home(
            &vars(&[("NISABA_HOME", "/x"), ("APPDATA", "/a")]),
            Some(d.path()),
            Some(Path::new("/h")),
            true,
        );
        assert_eq!(r, Some(PathBuf::from("/x")));
    }

    #[test]
    fn portable_solo_en_windows() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("portable"), "").unwrap();
        let v = vars(&[("APPDATA", "/a")]);
        assert_eq!(
            elegir_home(&v, Some(d.path()), Some(Path::new("/h")), true),
            Some(d.path().join("datos"))
        );
        assert_eq!(
            elegir_home(&v, Some(d.path()), Some(Path::new("/h")), false),
            Some(Path::new("/h").join(".local/share").join("nisaba"))
        );
    }

    #[test]
    fn windows_usa_appdata() {
        let r = elegir_home(
            &vars(&[("APPDATA", "/a"), ("XDG_DATA_HOME", "/d")]),
            None,
            Some(Path::new("/h")),
            true,
        );
        assert_eq!(r, Some(Path::new("/a").join("nisaba")));
    }

    #[test]
    fn windows_con_espacios_y_tildes() {
        let r = elegir_home(
            &vars(&[("APPDATA", "/tmp/José Núñez/AppData")]),
            None,
            None,
            true,
        )
        .unwrap();
        assert!(
            r.ends_with(Path::new("José Núñez").join("AppData").join("nisaba")),
            "{r:?}"
        );
    }

    #[test]
    fn linux_xdg_y_luego_personal() {
        let r = elegir_home(
            &vars(&[("XDG_DATA_HOME", "/d"), ("APPDATA", "/a")]),
            None,
            Some(Path::new("/h")),
            false,
        );
        assert_eq!(r, Some(Path::new("/d").join("nisaba")));
        let r = elegir_home(&vars(&[]), None, Some(Path::new("/h")), false);
        assert_eq!(r, Some(Path::new("/h").join(".local/share").join("nisaba")));
    }

    #[test]
    fn sin_personal_avisa_salvo_con_variables() {
        let e = validar_entorno(false, false, false).unwrap_err();
        assert!(e.contains("No encuentro tu carpeta personal"), "{e}");
        assert!(validar_entorno(true, false, false).is_err());
        assert!(validar_entorno(true, false, true).is_ok());
        assert!(validar_entorno(true, true, false).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn preparar_home_sin_permiso_avisa_con_la_ruta() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let ruta = d.path().join("nisaba");
        let e = preparar_home(&ruta).unwrap_err();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(e.starts_with("No puedo crear la carpeta de datos"), "{e}");
        assert!(e.contains(&ruta.display().to_string()), "{e}");
        assert!(preparar_home(&d.path().join("ok")).is_ok());
    }

    #[test]
    fn sin_personal_es_none() {
        assert_eq!(elegir_home(&vars(&[]), None, None, false), None);
    }
}
