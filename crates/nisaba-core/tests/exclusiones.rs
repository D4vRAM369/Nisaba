mod comun;

use comun::{correr, entorno, Entorno};
use nisaba_core::busqueda::{buscar, recientes, resolver, Filtros, Resolucion};
use nisaba_core::exclusiones::{anadir, cubre, lista, normalizar, quitar};
use nisaba_core::modelo::Fuente;
use nisaba_core::{lector, salud};
use std::fs;
use std::path::PathBuf;

const DEMO: &str = "/home/u/Proyectos/Demo";
const PRINCIPAL: &str = "a1b2c3d4-0000-4000-8000-000000000001";

fn contar(e: &Entorno, sql: &str) -> i64 {
    e.conn.query_row(sql, [], |f| f.get(0)).unwrap()
}

fn copia_principal(e: &Entorno) -> PathBuf {
    e.rutas.home.join(format!(
        "copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}.jsonl"
    ))
}

fn estado(e: &Entorno, nombre: &str) -> (String, Option<String>) {
    e.conn
        .query_row(
            "SELECT estado, cwd FROM archivos WHERE ruta LIKE '%' || ?1",
            [nombre],
            |f| Ok((f.get(0)?, f.get(1)?)),
        )
        .unwrap()
}

#[test]
fn normalizar_y_cubrir() {
    assert_eq!(normalizar("/home/u/P/", false), ["home", "u", "P"]);
    assert_eq!(normalizar("/home/u/./x/../P", false), ["home", "u", "P"]);
    assert_eq!(
        normalizar("C:\\Users\\Bri\\P\\", true),
        ["c:", "users", "bri", "p"]
    );
    assert_eq!(normalizar("C:/Users/Bri", false), ["C:", "Users", "Bri"]);
    assert!(cubre("/p/Mirlo/", "/p/Mirlo", false));
    assert!(cubre("/p/Mirlo", "/p/Mirlo/app/src", false));
    assert!(!cubre("/p/Mirlo", "/p/Mirlo-desktop", false));
    assert!(!cubre("/p/mirlo", "/p/Mirlo", false));
    assert!(cubre("C:\\P\\mirlo", "c:/p/Mirlo/x", true));
    assert!(!cubre("/p/Mirlo/app", "/p/Mirlo", false));
}

#[test]
fn excluir_no_afecta_a_carpeta_hermana() {
    let mut e = entorno();
    anadir(&e.conn, &e.rutas, "/home/u/Proyectos/Mirl", false).unwrap();
    correr(&mut e);
    assert_eq!(
        contar(
            &e,
            "SELECT count(*) FROM conversaciones WHERE fuente='codex'"
        ),
        1
    );
}

#[test]
fn excluir_antes_de_indexar_no_lee_ni_copia() {
    let mut e = entorno();
    assert_eq!(anadir(&e.conn, &e.rutas, DEMO, false).unwrap(), 0);
    correr(&mut e);
    assert_eq!(
        contar(
            &e,
            "SELECT count(*) FROM conversaciones WHERE fuente='claude-code'"
        ),
        0
    );
    assert_eq!(
        estado(&e, &format!("{PRINCIPAL}.jsonl")),
        ("excluida".into(), Some(DEMO.into()))
    );
    assert_eq!(estado(&e, "agent-zz.jsonl").0, "excluida");
    assert!(!copia_principal(&e).exists());
    assert_eq!(lista(&e.conn).unwrap(), [DEMO]);
    // Una segunda pasada no cambia nada.
    correr(&mut e);
    assert_eq!(
        contar(
            &e,
            "SELECT count(*) FROM conversaciones WHERE fuente='claude-code'"
        ),
        0
    );
}

#[test]
fn excluir_sin_borrar_oculta_en_busqueda_recientes_y_leer() {
    let mut e = entorno();
    correr(&mut e);
    assert_eq!(anadir(&e.conn, &e.rutas, DEMO, false).unwrap(), 2);
    correr(&mut e);
    let r = buscar(&e.conn, "PillMate", &Filtros::default(), "cli").unwrap();
    assert_eq!(r.total_sesiones, 0);
    let rec = recientes(&e.conn, &Filtros::default()).unwrap();
    assert!(rec.resultados.iter().all(|x| x.fuente == Fuente::Codex));
    assert_eq!(rec.total_sesiones, 1);
    assert_eq!(
        resolver(&e.conn, "cc-a1b2c3d4").unwrap(),
        Resolucion::Excluida
    );
    assert_eq!(
        resolver(&e.conn, "cc-agent-zz").unwrap(),
        Resolucion::Excluida
    );
    let id: i64 = e
        .conn
        .query_row(
            "SELECT id FROM conversaciones WHERE id_original=?1",
            [PRINCIPAL],
            |f| f.get(0),
        )
        .unwrap();
    assert!(lector::leer(&e.conn, id, &Default::default()).is_err());
    // Conservada en la tablilla.
    assert_eq!(contar(&e, "SELECT count(*) FROM conversaciones"), 3);
    assert!(copia_principal(&e).exists());
}

#[test]
fn excluir_con_borrar_elimina_y_cuenta() {
    let mut e = entorno();
    correr(&mut e);
    assert_eq!(anadir(&e.conn, &e.rutas, DEMO, true).unwrap(), 2);
    assert_eq!(contar(&e, "SELECT count(*) FROM conversaciones"), 1);
    assert_eq!(
        contar(
            &e,
            "SELECT count(*) FROM partes_fts WHERE partes_fts MATCH 'pillmate'"
        ),
        0
    );
    assert!(!copia_principal(&e).exists());
    assert!(!e
        .rutas
        .home
        .join(format!(
            "copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}/subagents/agent-zz.jsonl"
        ))
        .exists());
    correr(&mut e);
    assert_eq!(contar(&e, "SELECT count(*) FROM conversaciones"), 1);
}

#[test]
fn quitar_reindexa() {
    let mut e = entorno();
    anadir(&e.conn, &e.rutas, DEMO, false).unwrap();
    correr(&mut e);
    quitar(&e.conn, "/home/u/Proyectos/Demo/").unwrap();
    assert!(lista(&e.conn).unwrap().is_empty());
    correr(&mut e);
    assert_eq!(
        resolver(&e.conn, "cc-a1b2c3d4").unwrap(),
        Resolucion::Uno(
            e.conn
                .query_row(
                    "SELECT id FROM conversaciones WHERE id_original=?1",
                    [PRINCIPAL],
                    |f| f.get(0)
                )
                .unwrap()
        )
    );
    assert_eq!(estado(&e, &format!("{PRINCIPAL}.jsonl")).0, "ok");
}

fn subagente_con_otro_cwd(e: &Entorno) {
    let ruta = e.rutas.claude.join(format!(
        "-home-u-Proyectos-Demo/{PRINCIPAL}/subagents/agent-zz.jsonl"
    ));
    let t = fs::read_to_string(&ruta)
        .unwrap()
        .replace(DEMO, "/otro/sitio");
    fs::write(&ruta, t).unwrap();
}

#[test]
fn subagente_sigue_a_la_madre() {
    // Antes de indexar: no se lee ni se copia.
    let mut e = entorno();
    subagente_con_otro_cwd(&e);
    anadir(&e.conn, &e.rutas, DEMO, false).unwrap();
    correr(&mut e);
    assert_eq!(estado(&e, "agent-zz.jsonl").0, "excluida");

    // Ya indexado: queda oculto aunque su cwd sea otro.
    let mut e = entorno();
    subagente_con_otro_cwd(&e);
    correr(&mut e);
    assert_eq!(anadir(&e.conn, &e.rutas, DEMO, false).unwrap(), 2);
    assert_eq!(
        resolver(&e.conn, "cc-agent-zz").unwrap(),
        Resolucion::Excluida
    );
}

#[test]
fn salud_cuenta_excluidos() {
    let mut e = entorno();
    anadir(&e.conn, &e.rutas, DEMO, false).unwrap();
    correr(&mut e);
    let filas = salud::informe(&e.conn).unwrap();
    let cc = filas
        .iter()
        .find(|f| f.fuente == Fuente::ClaudeCode)
        .unwrap();
    assert_eq!(cc.excluidos, 2);
    assert!(cc.errores.iter().all(|(r, _)| r.ends_with("roto.jsonl")));
}
