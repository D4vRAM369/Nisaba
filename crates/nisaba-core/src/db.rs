use rusqlite::{Connection, ErrorCode, TransactionBehavior};
use std::path::Path;
use std::time::{Duration, Instant};

const VERSION: i64 = 2;
const ESPERA: Duration = Duration::from_secs(5);

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
    let mut con = Connection::open(ruta)?;
    con.busy_timeout(ESPERA)?;
    activar_wal(&con)?;
    con.pragma_update(None, "foreign_keys", true)?;
    con.execute_batch(ESQUEMA)?;
    migrar(&mut con)?;
    Ok(con)
}

/// Pasar a WAL pide acceso exclusivo y SQLite no aplica ahí `busy_timeout`: se reintenta a mano.
/// Solo compite la primera apertura; después el modo ya está guardado en el archivo.
fn activar_wal(con: &Connection) -> rusqlite::Result<()> {
    let limite = Instant::now() + ESPERA;
    loop {
        match con.pragma_update(None, "journal_mode", "WAL") {
            Err(e)
                if e.sqlite_error_code() == Some(ErrorCode::DatabaseBusy)
                    && Instant::now() < limite =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            r => return r,
        }
    }
}

/// Migraciones por `PRAGMA user_version`. Nunca borran contenido.
fn migrar(con: &mut Connection) -> rusqlite::Result<()> {
    if version(con)? >= VERSION {
        return Ok(());
    }
    // GUI y MCP pueden abrir la tablilla a la vez. `Immediate` reserva la escritura antes de
    // leer la versión: quien llegue segundo espera y ve lo que migró el primero.
    let tx = con.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let actual = version(&tx)?;
    if actual < 1 {
        // v1: carpeta de trabajo. Se vacían firma y huella para que la siguiente
        // pasada relea cada fuente (o su copia) y rellene `cwd`.
        tx.execute_batch(
            "ALTER TABLE conversaciones ADD COLUMN cwd TEXT;
             UPDATE archivos SET mtime = NULL;
             UPDATE conversaciones SET huella = NULL;",
        )?;
    }
    if actual < 2 {
        // v2: carpetas excluidas. `conversaciones.excluida` la mantiene `exclusiones`;
        // la vista es el único filtro que usan búsqueda, recientes y lector.
        tx.execute_batch(
            "CREATE TABLE exclusiones(ruta TEXT PRIMARY KEY, desde TEXT NOT NULL);
             ALTER TABLE archivos ADD COLUMN cwd TEXT;
             ALTER TABLE conversaciones ADD COLUMN excluida INTEGER NOT NULL DEFAULT 0;
             UPDATE archivos SET cwd = (SELECT c.cwd FROM conversaciones c WHERE c.ruta = archivos.ruta);
             CREATE VIEW conversaciones_visibles AS
               SELECT c.* FROM conversaciones c
               WHERE c.excluida = 0 AND NOT EXISTS
                 (SELECT 1 FROM conversaciones p WHERE p.id = c.padre_id AND p.excluida = 1);",
        )?;
    }
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()
}

fn version(con: &Connection) -> rusqlite::Result<i64> {
    con.query_row("PRAGMA user_version", [], |f| f.get(0))
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
    fn dos_procesos_abren_a_la_vez_una_tablilla_sin_migrar() {
        for _ in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let ruta = dir.path().join("tablilla.db");
            let salida = std::sync::Barrier::new(8);
            let errores: Vec<String> = std::thread::scope(|s| {
                let hilos: Vec<_> = (0..8)
                    .map(|_| {
                        s.spawn(|| {
                            salida.wait();
                            abrir(&ruta).err().map(|e| e.to_string())
                        })
                    })
                    .collect();
                hilos
                    .into_iter()
                    .filter_map(|h| h.join().unwrap())
                    .collect()
            });
            assert!(errores.is_empty(), "{errores:?}");
            let con = abrir(&ruta).unwrap();
            let version: i64 = con
                .query_row("PRAGMA user_version", [], |f| f.get(0))
                .unwrap();
            assert_eq!(version, 2);
        }
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
