//! La tablilla cifrada del archivo web: `tablilla-web.db`, abierta con la llave maestra.
//! Es la misma base que la local (mismo esquema y búsqueda), pero con SQLCipher.

use std::path::Path;

use rusqlite::{Connection, TransactionBehavior};

use crate::boveda::{LlaveMaestra, TABLILLA_WEB};
use crate::db;

/// La tablilla web va una versión por delante de la local (2): añade las versiones de una conversación.
const VERSION_WEB: i64 = 3;

pub fn existe(home: &Path) -> bool {
    home.join(TABLILLA_WEB).exists()
}

/// Abre (o crea) `tablilla-web.db` con la llave. Con una llave incorrecta falla aquí mismo.
pub fn abrir(home: &Path, llave: &LlaveMaestra) -> rusqlite::Result<Connection> {
    std::fs::create_dir_all(home)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let mut con = Connection::open(home.join(TABLILLA_WEB))?;
    poner_llave(&con, llave)?;
    con.pragma_update(None, "temp_store", "MEMORY")?;
    db::preparar(&mut con)?;
    migrar_web(&mut con)?;
    Ok(con)
}

/// Llave en bruto (`x'…'`): SQLCipher no la pasa por PBKDF2, que ya hizo Argon2 en la bóveda.
fn poner_llave(con: &Connection, llave: &LlaveMaestra) -> rusqlite::Result<()> {
    let sql = zeroize::Zeroizing::new(format!("PRAGMA key = \"x'{}'\"", llave.hex().as_str()));
    // El error de rusqlite puede arrastrar el SQL: se descarta para que la llave nunca salga en un mensaje.
    con.execute_batch(&sql)
        .map_err(|e| match e.sqlite_error() {
            Some(f) => {
                rusqlite::Error::SqliteFailure(*f, Some("no se pudo aplicar la llave".to_string()))
            }
            None => rusqlite::Error::InvalidQuery,
        })?;
    // `PRAGMA key` no comprueba nada: el primer acceso a una página es lo que falla con una
    // llave equivocada (`file is not a database`). Se fuerza aquí para no descubrirlo más tarde.
    con.query_row("SELECT count(*) FROM sqlite_master", [], |f| {
        f.get::<_, i64>(0)
    })?;
    Ok(())
}

/// v3: una conversación puede ser una versión anterior de otra (`version_de`) y no se lista.
/// Además guarda qué es cada fila (`tipo`), cuándo entró (`importada`) y su JSON (`originales`).
fn migrar_web(con: &mut Connection) -> rusqlite::Result<()> {
    if version(con)? >= VERSION_WEB {
        return Ok(());
    }
    // Igual que en `db::migrar`: `Immediate` hace que quien llegue segundo vea lo que migró el primero.
    let tx = con.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if version(&tx)? < VERSION_WEB {
        tx.execute_batch(
            "ALTER TABLE conversaciones ADD COLUMN version_de INTEGER REFERENCES conversaciones(id);
             ALTER TABLE conversaciones ADD COLUMN tipo TEXT NOT NULL DEFAULT 'conversacion';
             ALTER TABLE conversaciones ADD COLUMN importada TEXT;
             CREATE TABLE originales(
               conversacion_id INTEGER PRIMARY KEY REFERENCES conversaciones(id) ON DELETE CASCADE,
               json TEXT NOT NULL);
             DROP VIEW conversaciones_visibles;
             CREATE VIEW conversaciones_visibles AS
               SELECT c.* FROM conversaciones c
               WHERE c.excluida = 0 AND c.version_de IS NULL AND NOT EXISTS
                 (SELECT 1 FROM conversaciones p WHERE p.id = c.padre_id AND p.excluida = 1);",
        )?;
        tx.pragma_update(None, "user_version", VERSION_WEB)?;
    }
    tx.commit()
}

fn version(con: &Connection) -> rusqlite::Result<i64> {
    con.query_row("PRAGMA user_version", [], |f| f.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const MARCADOR: &str = "ZorroVioletaMarcador";

    fn sembrar(con: &Connection) {
        con.execute(
            "INSERT INTO conversaciones(id, fuente, id_original) VALUES (1, 'claude-web', 'abc')",
            [],
        )
        .unwrap();
        con.execute("INSERT INTO mensajes(id, conversacion_id, orden, rol, oculto) VALUES (1, 1, 0, 'usuario', 0)", []).unwrap();
        con.execute(
            "INSERT INTO partes(id, mensaje_id, orden, tipo, contenido) VALUES (1, 1, 0, 'texto', ?1)",
            [format!("La Medicación de PillMate {MARCADOR}")],
        )
        .unwrap();
    }

    fn coincidencias(con: &Connection, q: &str) -> i64 {
        con.query_row(
            "SELECT count(*) FROM partes_fts WHERE partes_fts MATCH ?1",
            [q],
            |f| f.get(0),
        )
        .unwrap()
    }

    fn contiene(bytes: &[u8], aguja: &[u8]) -> bool {
        bytes.windows(aguja.len()).any(|v| v == aguja)
    }

    #[test]
    fn sqlcipher_esta_enlazado() {
        let con = Connection::open_in_memory().unwrap();
        let v: String = con
            .query_row("PRAGMA cipher_version", [], |f| f.get(0))
            .unwrap_or_default();
        assert!(!v.is_empty());
    }

    #[test]
    fn marcador_no_aparece_en_disco() {
        let dir = tempdir().unwrap();
        let con = abrir(dir.path(), &LlaveMaestra::de_prueba(7)).unwrap();
        sembrar(&con);
        // Con la conexión abierta parte de lo escrito sigue en el WAL: se mira también.
        let db = std::fs::read(dir.path().join(TABLILLA_WEB)).unwrap();
        let wal = std::fs::read(dir.path().join(format!("{TABLILLA_WEB}-wal"))).unwrap();
        assert!(db.len() + wal.len() > 0);
        for (nombre, bytes) in [("db", db), ("wal", wal)] {
            assert!(
                !contiene(&bytes, MARCADOR.as_bytes()),
                "texto claro en {nombre}"
            );
            assert!(
                !contiene(&bytes, b"SQLite format 3"),
                "cabecera clara en {nombre}"
            );
        }
        drop(con);
        let db = std::fs::read(dir.path().join(TABLILLA_WEB)).unwrap();
        assert!(!contiene(&db, MARCADOR.as_bytes()));
    }

    #[test]
    fn llave_incorrecta_falla() {
        let dir = tempdir().unwrap();
        drop(abrir(dir.path(), &LlaveMaestra::de_prueba(1)).unwrap());
        let mala = LlaveMaestra::de_prueba(2);
        let e = abrir(dir.path(), &mala).unwrap_err().to_string();
        assert!(e.contains("not a database"), "{e}");
        assert!(!e.contains(mala.hex().as_str()));
    }

    #[test]
    fn reabrir_conserva_y_busca() {
        let dir = tempdir().unwrap();
        let llave = LlaveMaestra::de_prueba(3);
        assert!(!existe(dir.path()));
        let con = abrir(dir.path(), &llave).unwrap();
        sembrar(&con);
        drop(con);
        assert!(existe(dir.path()));
        let con = abrir(dir.path(), &llave).unwrap();
        assert_eq!(coincidencias(&con, "pillmate"), 1);
        assert_eq!(version(&con).unwrap(), VERSION_WEB);
    }

    #[test]
    fn abre_dos_veces_a_la_vez() {
        let llave = LlaveMaestra::de_prueba(4);
        for _ in 0..10 {
            let dir = tempdir().unwrap();
            let salida = std::sync::Barrier::new(8);
            let errores: Vec<String> = std::thread::scope(|s| {
                let hilos: Vec<_> = (0..8)
                    .map(|_| {
                        s.spawn(|| {
                            salida.wait();
                            abrir(dir.path(), &llave).err().map(|e| e.to_string())
                        })
                    })
                    .collect();
                hilos
                    .into_iter()
                    .filter_map(|h| h.join().unwrap())
                    .collect()
            });
            assert!(errores.is_empty(), "{errores:?}");
            let con = abrir(dir.path(), &llave).unwrap();
            assert_eq!(version(&con).unwrap(), VERSION_WEB);
        }
    }

    #[test]
    fn busqueda_acentos_igual_que_local() {
        let dir = tempdir().unwrap();
        let con = abrir(dir.path(), &LlaveMaestra::de_prueba(5)).unwrap();
        sembrar(&con);
        assert_eq!(coincidencias(&con, "medicacion"), 1);
        assert_eq!(coincidencias(&con, "PILLMATE"), 1);
    }

    #[test]
    fn versiones_no_son_visibles() {
        let dir = tempdir().unwrap();
        let con = abrir(dir.path(), &LlaveMaestra::de_prueba(6)).unwrap();
        sembrar(&con);
        con.execute(
            "INSERT INTO conversaciones(id, fuente, id_original, version_de) VALUES (2, 'claude-web', 'abc-v1', 1)",
            [],
        )
        .unwrap();
        let mut q = con
            .prepare("SELECT id FROM conversaciones_visibles ORDER BY id")
            .unwrap();
        let visibles: Vec<i64> = q
            .query_map([], |f| f.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(visibles, vec![1]);
    }

    #[test]
    fn la_llave_se_aplica_en_bruto_y_no_como_frase() {
        let dir = tempdir().unwrap();
        let llave = LlaveMaestra::de_prueba(8);
        let con = abrir(dir.path(), &llave).unwrap();
        sembrar(&con);
        drop(con);
        // Una llave en bruto ignora `kdf_iter`; una frase derivaría otra llave y no abriría.
        let con = Connection::open(dir.path().join(TABLILLA_WEB)).unwrap();
        con.execute_batch(&format!("PRAGMA key = \"x'{}'\"", llave.hex().as_str()))
            .unwrap();
        con.execute_batch("PRAGMA kdf_iter = 1234").unwrap();
        let n: i64 = con
            .query_row("SELECT count(*) FROM conversaciones", [], |f| f.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn la_tablilla_local_no_sube_de_version() {
        let dir = tempdir().unwrap();
        let con = db::abrir(&dir.path().join("t.db")).unwrap();
        assert_eq!(version(&con).unwrap(), 2);
    }
}
