mod comun;

use comun::{correr, entorno, Entorno};
use nisaba_core::busqueda::{buscar, consulta_fts, resolver_id, Filtros};

fn indexado() -> Entorno {
    let mut e = entorno();
    correr(&mut e);
    e
}

#[test]
fn consulta_escapa_especiales() {
    assert_eq!(consulta_fts("c++ foo-bar AND"), Some("\"c++\" \"foo-bar\" \"AND\"".into()));
    assert_eq!(consulta_fts("\"sin cerrar"), Some("\"sin cerrar\"".into()));
    assert_eq!(consulta_fts("medica*"), Some("\"medica\"*".into()));
    assert_eq!(consulta_fts(" * "), None);
    assert_eq!(consulta_fts("\"una frase\" otro"), Some("\"una frase\" \"otro\"".into()));
}

#[test]
fn especiales_no_rompen_sql() {
    let e = indexado();
    assert!(buscar(&e.conn, "c++ : \" -x", &Filtros::default(), "cli").is_ok());
}

#[test]
fn encuentra_sin_acentos() {
    let e = indexado();
    let r = buscar(&e.conn, "subtitulos", &Filtros::default(), "cli").unwrap();
    assert_eq!(r.resultados.len(), 1);
    assert_eq!(r.resultados[0].id_publico, "cx-cx01");
    assert!(r.resultados[0].fragmento.contains("«subtítulos»"), "{}", r.resultados[0].fragmento);
}

#[test]
fn razonamiento_solo_si_se_pide() {
    let e = indexado();
    let n = |razonamiento| {
        let f = Filtros { razonamiento, ..Default::default() };
        buscar(&e.conn, "SRT", &f, "cli").unwrap().resultados.len()
    };
    assert_eq!((n(false), n(true)), (0, 1));
}

#[test]
fn herramientas_solo_si_se_pide() {
    let e = indexado();
    let n = |codigo| {
        let f = Filtros { codigo, ..Default::default() };
        buscar(&e.conn, "zanahoria42", &f, "cli").unwrap().resultados.len()
    };
    assert_eq!((n(false), n(true)), (0, 1));
}

#[test]
fn ocultos_no_cuentan() {
    let e = indexado();
    let f = Filtros { razonamiento: true, codigo: true, ..Default::default() };
    assert_eq!(buscar(&e.conn, "ornitorrinco", &f, "cli").unwrap().resultados.len(), 0);
}

#[test]
fn subagente_agrupa_en_madre() {
    let e = indexado();
    let r = buscar(&e.conn, "pillmate", &Filtros::default(), "cli").unwrap();
    assert_eq!(r.resultados.len(), 1);
    assert_eq!(r.resultados[0].id_publico, "cc-a1b2c3d4");
    assert!(r.resultados[0].coincidencias >= 2);
    assert_eq!(r.total_sesiones, 1);
}

#[test]
fn filtros_fuente_proyecto_desde() {
    use nisaba_core::modelo::Fuente;
    let e = indexado();
    let q = |f: Filtros| buscar(&e.conn, "pillmate", &f, "cli").unwrap().resultados.len();
    assert_eq!(q(Filtros { fuente: Some(Fuente::Codex), ..Default::default() }), 0);
    assert_eq!(q(Filtros { fuente: Some(Fuente::ClaudeCode), ..Default::default() }), 1);
    assert_eq!(q(Filtros { proyecto: Some("Otro".into()), ..Default::default() }), 0);
    assert_eq!(q(Filtros { proyecto: Some("Demo".into()), ..Default::default() }), 1);
    assert_eq!(q(Filtros { desde: Some("2999-01-01".into()), ..Default::default() }), 0);
}

#[test]
fn sugiere_traduccion() {
    let e = indexado();
    let r = buscar(&e.conn, "horario", &Filtros::default(), "cli").unwrap();
    assert_eq!(r.resultados.len(), 0);
    assert_eq!(r.sugerencia.as_deref(), Some("Sin resultados. ¿Probar «schedule»?"));
}

#[test]
fn resuelve_id_publico() {
    let e = indexado();
    let id = resolver_id(&e.conn, "cc-a1b2c3d4").unwrap().unwrap();
    let orig: String = e
        .conn
        .query_row("SELECT id_original FROM conversaciones WHERE id=?1", [id], |f| f.get(0))
        .unwrap();
    assert!(orig.starts_with("a1b2c3d4"));
    assert!(resolver_id(&e.conn, "cx-cx01").unwrap().is_some());
    assert!(resolver_id(&e.conn, "cx-a1b2c3d4").unwrap().is_none());
    assert!(resolver_id(&e.conn, "zz-1234").unwrap().is_none());
    assert!(resolver_id(&e.conn, "basura").unwrap().is_none());
}

#[test]
fn serializa_a_json() {
    let e = indexado();
    let r = buscar(&e.conn, "subtitulos", &Filtros::default(), "cli").unwrap();
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["resultados"][0]["fuente"], "codex");
}

#[test]
fn cuenta_uso() {
    let e = indexado();
    buscar(&e.conn, "pillmate", &Filtros::default(), "cli").unwrap();
    buscar(&e.conn, "pillmate", &Filtros::default(), "cli").unwrap();
    let n: i64 = e
        .conn
        .query_row(
            "SELECT busquedas FROM uso WHERE dia = date('now','localtime') AND canal='cli'",
            [],
            |f| f.get(0),
        )
        .unwrap();
    assert_eq!(n, 2);
}
