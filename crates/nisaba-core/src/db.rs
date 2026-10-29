use rusqlite::Connection;
use std::path::Path;

const ESQUEMA: &str = "
CREATE TABLE IF NOT EXISTS conversaciones(
  id INTEGER PRIMARY KEY, fuente TEXT NOT NULL, id_original TEXT NOT NULL,
  padre_id INTEGER REFERENCES conversaciones(id), descripcion TEXT,
  proyecto TEXT, titulo TEXT, inicio TEXT, fin TEXT, hoja TEXT,
  ruta TEXT, huella TEXT, UNIQUE(fuente, id_original));
CREATE TABLE IF NOT EXISTS mensajes(
  id INTEGER PRIMARY KEY, conversacion_id INTEGER NOT NULL REFERENCES conversaciones(id) ON DELETE CASCADE,
  orden INTEGER NOT NULL, uuid TEXT, id_padre TEXT, rol TEXT NOT NULL, modelo TEXT, hora TEXT, oculto INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS partes(
  id INTEGER PRIMARY KEY, mensaje_id INTEGER NOT NULL REFERENCES mensajes(id) ON DELETE CASCADE,
  orden INTEGER NOT NULL, tipo TEXT NOT NULL, contenido TEXT NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS partes_fts USING fts5(
  contenido, content='partes', content_rowid='id', tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER IF NOT EXISTS partes_ai AFTER INSERT ON partes BEGIN
  INSERT INTO partes_fts(rowid, contenido) VALUES (new.id, new.contenido);
END;
CREATE TRIGGER IF NOT EXISTS partes_ad AFTER DELETE ON partes BEGIN
  INSERT INTO partes_fts(partes_fts, rowid, contenido) VALUES ('delete', old.id, old.contenido);
END;
CREATE INDEX IF NOT EXISTS mensajes_conv ON mensajes(conversacion_id, orden);
CREATE INDEX IF NOT EXISTS partes_msg ON partes(mensaje_id, orden);
CREATE TABLE IF NOT EXISTS archivos(
  ruta TEXT PRIMARY KEY, fuente TEXT NOT NULL, tamano INTEGER, mtime INTEGER,
  estado TEXT NOT NULL, error TEXT, avisos INTEGER NOT NULL DEFAULT 0, visto TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS uso(dia TEXT NOT NULL, canal TEXT NOT NULL, busquedas INTEGER NOT NULL, PRIMARY KEY(dia, canal));
";

/// Abre (o crea) la tablilla: carpeta, WAL, busy_timeout, foreign_keys y esquema.
pub fn abrir(ruta: &Path) -> rusqlite::Result<Connection> {
    if let Some(dir) = ruta.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    }
    let con = Connection::open(ruta)?;
    con.busy_timeout(std::time::Duration::from_millis(5000))?;
    con.pragma_update(None, "journal_mode", "WAL")?;
    con.pragma_update(None, "foreign_keys", true)?;
    con.execute_batch(ESQUEMA)?;
    Ok(con)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn sembrar(con: &Connection) {
        con.execute(
            "INSERT INTO conversaciones(id, fuente, id_original) VALUES (1, 'claude-code', 'abc')",
            [],
        )
        .unwrap();
        con.execute("INSERT INTO mensajes(id, conversacion_id, orden, rol, oculto) VALUES (1, 1, 0, 'usuario', 0)", []).unwrap();
        con.execute("INSERT INTO partes(id, mensaje_id, orden, tipo, contenido) VALUES (1, 1, 0, 'texto', 'La Medicación de PillMate')", []).unwrap();
    }

    fn coincidencias(con: &Connection, q: &str) -> i64 {
        con.query_row(
            "SELECT count(*) FROM partes_fts WHERE partes_fts MATCH ?1",
            [q],
            |f| f.get(0),
        )
        .unwrap()
    }

    #[test]
    fn fts_ignora_acentos_y_mayusculas() {
        let dir = tempfile::tempdir().unwrap();
        let con = abrir(&dir.path().join("sub/tablilla.db")).unwrap();
        sembrar(&con);
        assert_eq!(coincidencias(&con, "medicacion"), 1);
        assert_eq!(coincidencias(&con, "pillmate"), 1);
    }

    #[test]
    fn borrar_conversacion_borra_fts() {
        let dir = tempfile::tempdir().unwrap();
        let con = abrir(&dir.path().join("tablilla.db")).unwrap();
        sembrar(&con);
        con.execute("DELETE FROM conversaciones", []).unwrap();
        assert_eq!(coincidencias(&con, "medicacion"), 0);
    }
}
