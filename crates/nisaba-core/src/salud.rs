use crate::modelo::Fuente;
use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct FilaSalud {
    pub fuente: Fuente,
    pub ultima: Option<String>,
    pub vistos: i64,
    pub parseados: i64,
    pub errores: Vec<(String, String)>,
    pub avisos: i64,
}

/// Una fila por fuente (siempre las dos, aunque no tengan archivos).
pub fn informe(conn: &Connection) -> rusqlite::Result<Vec<FilaSalud>> {
    let mut filas = Vec::new();
    for fuente in [Fuente::ClaudeCode, Fuente::Codex] {
        let f = fuente.as_str();
        let (vistos, parseados, avisos): (i64, i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(estado='ok'),0), COALESCE(SUM(avisos),0)
             FROM archivos WHERE fuente = ?1",
            [f],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut st = conn.prepare(
            "SELECT ruta, COALESCE(error,'') FROM archivos
             WHERE fuente = ?1 AND estado = 'error' ORDER BY ruta",
        )?;
        let errores = st
            .query_map([f], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let ultima: Option<String> = conn.query_row(
            "SELECT MAX(fin) FROM conversaciones WHERE fuente = ?1",
            [f],
            |r| r.get(0),
        )?;
        filas.push(FilaSalud { fuente, ultima, vistos, parseados, errores, avisos });
    }
    Ok(filas)
}

/// Aviso si Claude Code va a borrar sesiones pronto (`cleanupPeriodDays` ausente o <= 30).
/// Un archivo que falta o no se puede leer cuenta como "falta".
pub fn aviso_limpieza_claude(settings: &Path) -> Option<String> {
    let dias = std::fs::read_to_string(settings)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("cleanupPeriodDays").and_then(|d| d.as_i64()));
    let n = dias.unwrap_or(30);
    if n > 30 {
        return None;
    }
    Some(format!(
        "Claude Code borra sesiones de más de {n} días. Sube cleanupPeriodDays en ~/.claude/settings.json."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con(contenido: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, contenido).unwrap();
        (d, p)
    }

    #[test]
    fn aviso_limpieza() {
        let (_d, p) = con("{}");
        let a = aviso_limpieza_claude(&p).unwrap();
        assert!(a.contains("30 días"), "{a}");
        let (_d, p) = con(r#"{"cleanupPeriodDays":36500}"#);
        assert_eq!(aviso_limpieza_claude(&p), None);
        let (_d, p) = con(r#"{"cleanupPeriodDays":7}"#);
        assert!(aviso_limpieza_claude(&p).unwrap().contains("7 días"));
        assert!(aviso_limpieza_claude(Path::new("/no/existe.json")).is_some());
        let (_d, p) = con("no es json");
        assert!(aviso_limpieza_claude(&p).is_some());
    }
}
