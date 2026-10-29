#![allow(dead_code)]
use nisaba_core::db;
use nisaba_core::indexador::{indexar, Progreso, ResultadoIndexado, Rutas};
use rusqlite::Connection;
use std::fs;
use std::path::Path;

pub fn copiar(origen: &Path, destino: &Path) {
    fs::create_dir_all(destino).unwrap();
    for e in fs::read_dir(origen).unwrap() {
        let e = e.unwrap();
        let d = destino.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copiar(&e.path(), &d);
        } else {
            fs::copy(e.path(), d).unwrap();
        }
    }
}

pub struct Entorno {
    pub _dir: tempfile::TempDir,
    pub rutas: Rutas,
    pub conn: Connection,
}

pub fn entorno() -> Entorno {
    let dir = tempfile::tempdir().unwrap();
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let claude = dir.path().join("claude");
    let codex = dir.path().join("codex");
    copiar(&fx.join("claude"), &claude);
    copiar(&fx.join("codex"), &codex);
    let home = dir.path().join("home");
    let conn = db::abrir(&home.join("tablilla.db")).unwrap();
    Entorno {
        rutas: Rutas {
            home,
            claude,
            codex,
        },
        conn,
        _dir: dir,
    }
}

pub fn correr(e: &mut Entorno) -> ResultadoIndexado {
    indexar(&mut e.conn, &e.rutas, &mut |_p: Progreso| {})
}
