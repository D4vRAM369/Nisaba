use nisaba_core::indexador::ResultadoIndexado;
use nisaba_core::modelo::{Mensaje, Parte, Rol, TipoParte};
use nisaba_core::titulo::titulo_util;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
mod comun;
use comun::{correr, entorno, Entorno};

const PRINCIPAL: &str = "a1b2c3d4-0000-4000-8000-000000000001";

fn contar(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |f| f.get(0)).unwrap()
}

fn ruta_principal(e: &Entorno) -> PathBuf {
    e.rutas
        .claude
        .join("-home-u-Proyectos-Demo")
        .join(format!("{PRINCIPAL}.jsonl"))
}

#[test]
fn indexa_fixtures() {
    let mut e = entorno();
    let r = correr(&mut e);
    assert!(matches!(r, ResultadoIndexado::Hecho { nuevos: 3, errores: 1, .. }), "{r:?}");
    assert_eq!(contar(&e.conn, "SELECT count(*) FROM conversaciones"), 3);
    assert_eq!(contar(&e.conn, "SELECT count(*) FROM conversaciones WHERE fuente='codex'"), 1);
    assert_eq!(contar(&e.conn, "SELECT count(*) FROM archivos WHERE estado='error'"), 1);
    let ok: i64 = contar(&e.conn, "SELECT count(*) FROM archivos WHERE estado='ok'");
    assert_eq!(ok, 4); // principal, subagente, 2 codex
    let (padre, id_principal): (Option<i64>, i64) = (
        e.conn
            .query_row("SELECT padre_id FROM conversaciones WHERE id_original='agent-zz'", [], |f| f.get(0))
            .unwrap(),
        e.conn
            .query_row("SELECT id FROM conversaciones WHERE id_original=?1", [PRINCIPAL], |f| f.get(0))
            .unwrap(),
    );
    assert_eq!(padre, Some(id_principal));
    let proyecto: String = e
        .conn
        .query_row("SELECT proyecto FROM conversaciones WHERE id_original=?1", [PRINCIPAL], |f| f.get(0))
        .unwrap();
    assert_eq!(proyecto, "Demo");
    assert!(e.rutas.home.join(format!("copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}.jsonl")).exists());
}

#[test]
fn segunda_pasada_no_reescribe() {
    let mut e = entorno();
    correr(&mut e);
    let r = correr(&mut e);
    assert!(matches!(r, ResultadoIndexado::Hecho { nuevos: 0, cambiados: 0, .. }), "{r:?}");
}

#[test]
fn archivo_crece_se_actualiza_con_id_estable() {
    let mut e = entorno();
    correr(&mut e);
    let sql = "SELECT id FROM conversaciones WHERE id_original=?1";
    let id1: i64 = e.conn.query_row(sql, [PRINCIPAL], |f| f.get(0)).unwrap();
    let n1 = contar(&e.conn, &format!("SELECT count(*) FROM mensajes WHERE conversacion_id={id1}"));
    let ruta = ruta_principal(&e);
    let original = fs::read_to_string(&ruta).unwrap();
    // El fixture acaba en una línea cortada (escritura en curso): se descarta y se añade una completa.
    let mut s = original.trim_end_matches('\n')[..original.trim_end_matches('\n').rfind('\n').unwrap() + 1].to_string();
    s.push_str(&format!("{{\"type\":\"user\",\"uuid\":\"u99\",\"parentUuid\":\"a2\",\"sessionId\":\"{PRINCIPAL}\",\"cwd\":\"/home/u/Proyectos/Demo\",\"timestamp\":\"2026-10-29T10:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"Una pregunta nueva sobre el esquema\"}}}}\n"));
    fs::write(&ruta, s).unwrap();
    let r = correr(&mut e);
    assert!(matches!(r, ResultadoIndexado::Hecho { nuevos: 0, cambiados: 1, .. }), "{r:?}");
    let id2: i64 = e.conn.query_row(sql, [PRINCIPAL], |f| f.get(0)).unwrap();
    assert_eq!(id1, id2);
    let n2 = contar(&e.conn, &format!("SELECT count(*) FROM mensajes WHERE conversacion_id={id2}"));
    assert_eq!(n2, n1 + 1);
    assert_eq!(contar(&e.conn, "SELECT count(*) FROM partes_fts WHERE partes_fts MATCH 'esquema'"), 1);
}

#[test]
fn fuente_borrada_se_conserva() {
    let mut e = entorno();
    correr(&mut e);
    let antes = contar(&e.conn, "SELECT count(*) FROM mensajes");
    fs::remove_file(ruta_principal(&e)).unwrap();
    let r = correr(&mut e);
    assert!(matches!(r, ResultadoIndexado::Hecho { .. }));
    assert_eq!(contar(&e.conn, "SELECT count(*) FROM mensajes"), antes);
    assert_eq!(
        contar(&e.conn, &format!("SELECT count(*) FROM conversaciones WHERE id_original='{PRINCIPAL}'")),
        1
    );
    assert!(e.rutas.home.join(format!("copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}.jsonl")).exists());
}

#[test]
fn ocupado_si_otro_indexa() {
    let mut e = entorno();
    fs::create_dir_all(&e.rutas.home).unwrap();
    let f = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(e.rutas.home.join("index.lock"))
        .unwrap();
    f.try_lock().unwrap();
    assert!(matches!(correr(&mut e), ResultadoIndexado::Ocupado));
    assert_eq!(contar(&e.conn, "SELECT count(*) FROM archivos"), 0);
    drop(f);
    assert!(matches!(correr(&mut e), ResultadoIndexado::Hecho { .. }));
}

#[test]
fn error_conserva_conversacion_anterior() {
    let mut e = entorno();
    correr(&mut e);
    let ruta = ruta_principal(&e);
    fs::write(&ruta, "{esto no es json\n{\"type\":\"user\"}\n").unwrap();
    let r = correr(&mut e);
    assert!(matches!(r, ResultadoIndexado::Hecho { errores: 1, .. }), "{r:?}");
    assert!(contar(&e.conn, &format!("SELECT count(*) FROM mensajes m JOIN conversaciones c ON c.id=m.conversacion_id WHERE c.id_original='{PRINCIPAL}'")) > 0);
}

fn usuario(texto: &str) -> Mensaje {
    Mensaje {
        uuid: None,
        id_padre: None,
        rol: Rol::Usuario,
        modelo: None,
        hora: None,
        oculto: false,
        partes: vec![Parte { tipo: TipoParte::Texto, contenido: texto.into() }],
    }
}

#[test]
fn titulo_salta_genericos() {
    let ms = vec![usuario("continúa"), usuario("/compact"), usuario("Arregla el parseo de subtítulos en Mirlo")];
    assert_eq!(
        titulo_util(Some("Mirlo"), &ms),
        "Mirlo · Arregla el parseo de subtítulos en Mirlo"
    );
    assert_eq!(titulo_util(None, &[]), "sin proyecto · (sin título)");
    assert_eq!(titulo_util(Some("X"), &[usuario("ok")]), "X · (sin título)");
}

#[test]
fn copia_fallida_se_reintenta() {
    let mut e = entorno();
    fs::create_dir_all(e.rutas.home.join("copias")).unwrap();
    fs::write(e.rutas.home.join("copias/claude-code"), "obstáculo").unwrap();
    let r = correr(&mut e);
    assert!(matches!(r, ResultadoIndexado::Hecho { errores, .. } if errores >= 1), "{r:?}");
    let estado: String = e
        .conn
        .query_row("SELECT estado FROM archivos WHERE ruta=?1", [ruta_principal(&e).to_string_lossy()], |f| f.get(0))
        .unwrap();
    assert_eq!(estado, "error");
    let msg: String = e
        .conn
        .query_row("SELECT error FROM archivos WHERE ruta=?1", [ruta_principal(&e).to_string_lossy()], |f| f.get(0))
        .unwrap();
    assert!(msg.starts_with("copia: "), "{msg}");
    fs::remove_file(e.rutas.home.join("copias/claude-code")).unwrap();
    correr(&mut e);
    assert!(e.rutas.home.join(format!("copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}.jsonl")).exists());
    assert!(e.rutas.home.join(format!("copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}/subagents/agent-zz.jsonl")).exists());
}

#[test]
fn fallo_si_home_es_un_archivo() {
    let mut e = entorno();
    let f = e._dir.path().join("archivo");
    fs::write(&f, "x").unwrap();
    e.rutas.home = f;
    assert!(matches!(correr(&mut e), ResultadoIndexado::Fallo(_)));
}
