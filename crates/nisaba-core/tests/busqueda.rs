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
    assert_eq!(
        consulta_fts("c++ foo-bar AND"),
        Some("\"c++\" \"foo-bar\" \"AND\"".into())
    );
    assert_eq!(consulta_fts("\"sin cerrar"), Some("\"sin cerrar\"".into()));
    assert_eq!(consulta_fts("medica*"), Some("\"medica\"*".into()));
    assert_eq!(consulta_fts(" * "), None);
    assert_eq!(
        consulta_fts("\"una frase\" otro"),
        Some("\"una frase\" \"otro\"".into())
    );
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
    assert!(
        r.resultados[0].fragmento.contains("«subtítulos»"),
        "{}",
        r.resultados[0].fragmento
    );
}

#[test]
fn razonamiento_solo_si_se_pide() {
    let e = indexado();
    let n = |razonamiento| {
        let f = Filtros {
            razonamiento,
            ..Default::default()
        };
        buscar(&e.conn, "SRT", &f, "cli").unwrap().resultados.len()
    };
    assert_eq!((n(false), n(true)), (0, 1));
}

#[test]
fn herramientas_solo_si_se_pide() {
    let e = indexado();
    let n = |codigo| {
        let f = Filtros {
            codigo,
            ..Default::default()
        };
        buscar(&e.conn, "zanahoria42", &f, "cli")
            .unwrap()
            .resultados
            .len()
    };
    assert_eq!((n(false), n(true)), (0, 1));
}

#[test]
fn ocultos_no_cuentan() {
    let e = indexado();
    let f = Filtros {
        razonamiento: true,
        codigo: true,
        ..Default::default()
    };
    assert_eq!(
        buscar(&e.conn, "ornitorrinco", &f, "cli")
            .unwrap()
            .resultados
            .len(),
        0
    );
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
    let q = |f: Filtros| {
        buscar(&e.conn, "pillmate", &f, "cli")
            .unwrap()
            .resultados
            .len()
    };
    assert_eq!(
        q(Filtros {
            fuente: Some(Fuente::Codex),
            ..Default::default()
        }),
        0
    );
    assert_eq!(
        q(Filtros {
            fuente: Some(Fuente::ClaudeCode),
            ..Default::default()
        }),
        1
    );
    assert_eq!(
        q(Filtros {
            proyecto: Some("Otro".into()),
            ..Default::default()
        }),
        0
    );
    assert_eq!(
        q(Filtros {
            proyecto: Some("Demo".into()),
            ..Default::default()
        }),
        1
    );
    assert_eq!(
        q(Filtros {
            desde: Some("2999-01-01".into()),
            ..Default::default()
        }),
        0
    );
}

#[test]
fn sugiere_traduccion() {
    let e = indexado();
    let r = buscar(&e.conn, "horario", &Filtros::default(), "cli").unwrap();
    assert_eq!(r.resultados.len(), 0);
    assert_eq!(
        r.sugerencia.as_deref(),
        Some("Sin resultados. ¿Probar «schedule»?")
    );
}

#[test]
fn resuelve_id_publico() {
    let e = indexado();
    let id = resolver_id(&e.conn, "cc-a1b2c3d4").unwrap().unwrap();
    let orig: String = e
        .conn
        .query_row(
            "SELECT id_original FROM conversaciones WHERE id=?1",
            [id],
            |f| f.get(0),
        )
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

#[test]
fn ids_publicos_distinguen_uuid7_y_subagentes() {
    use nisaba_core::busqueda::id_publico;
    use nisaba_core::modelo::Fuente;
    // UUIDv7 inventados: comparten los primeros 8 caracteres.
    let a = "01a17580-0000-7000-8000-aaaaaaaaaaa1";
    let b = "01a17580-0000-7000-8000-bbbbbbbbbbb2";
    assert_ne!(id_publico(Fuente::Codex, a), id_publico(Fuente::Codex, b));
    assert_eq!(id_publico(Fuente::Codex, a), "cx-aaaaaaa1");
    assert_eq!(
        id_publico(Fuente::ClaudeCode, "a1b2c3d4-0000"),
        "cc-a1b2c3d4"
    );
    assert_eq!(
        id_publico(Fuente::ClaudeCode, "agent-ab12cd34ef56"),
        "cc-cd34ef56"
    );

    let e = indexado();
    for (f, id) in [
        ("codex", a),
        ("codex", b),
        ("claude-code", "agent-ab12cd34ef56"),
    ] {
        e.conn
            .execute(
                "INSERT INTO conversaciones(fuente, id_original) VALUES (?1, ?2)",
                [f, id],
            )
            .unwrap();
    }
    let orig = |publico: &str| -> String {
        let id = resolver_id(&e.conn, publico).unwrap().unwrap();
        e.conn
            .query_row(
                "SELECT id_original FROM conversaciones WHERE id=?1",
                [id],
                |f| f.get(0),
            )
            .unwrap()
    };
    assert_eq!(orig(&id_publico(Fuente::Codex, a)), a);
    assert_eq!(orig(&id_publico(Fuente::Codex, b)), b);
    assert_eq!(orig("cc-cd34ef56"), "agent-ab12cd34ef56");
}

#[test]
fn resolver_respeta_el_extremo_de_cada_fila_y_detecta_ambiguedad() {
    use nisaba_core::busqueda::{resolver, Resolucion};
    let e = indexado();
    let ins = |f: &str, id: &str| {
        e.conn
            .execute(
                "INSERT INTO conversaciones(fuente, id_original) VALUES (?1, ?2)",
                [f, id],
            )
            .unwrap();
        e.conn.last_insert_rowid()
    };
    // A (Codex, por el final) acaba en "deadbeef"; B (Claude normal, por el principio)
    // empieza por "deadbeef": en su fuente cada uno solo responde a su extremo.
    let a = ins("codex", "01a00000-0000-7000-8000-0000deadbeef");
    let _otro = ins("codex", "deadbeef-0000-7000-8000-000000000000");
    let b = ins("claude-code", "deadbeef-1111-4000-8000-000000000000");
    let _c = ins("claude-code", "11111111-1111-4000-8000-0000deadbeef");
    assert_eq!(
        resolver(&e.conn, "cx-deadbeef").unwrap(),
        Resolucion::Uno(a)
    );
    assert_eq!(
        resolver(&e.conn, "cc-deadbeef").unwrap(),
        Resolucion::Uno(b)
    );
    // Dos filas con el mismo final: ambiguo, no se elige.
    ins("codex", "01a11111-0000-7000-8000-1111deadbeef");
    match resolver(&e.conn, "cx-deadbeef").unwrap() {
        Resolucion::Ambiguo(v) => assert_eq!(v.len(), 2),
        o => panic!("{o:?}"),
    }
    assert!(resolver_id(&e.conn, "cx-deadbeef").unwrap().is_none());
}

#[test]
fn recientes_lista_sesiones_madre_por_fecha_sin_contar_busquedas() {
    use nisaba_core::busqueda::{recientes, recientes_desde};
    let e = indexado();
    e.conn
        .execute(
            "UPDATE conversaciones SET fin='2026-10-01' WHERE fuente='codex'",
            [],
        )
        .unwrap();
    e.conn
        .execute(
            "UPDATE conversaciones SET fin='2026-10-02' WHERE fuente='claude-code'",
            [],
        )
        .unwrap();
    let f = Filtros {
        limite: 1,
        ..Default::default()
    };
    let primera = recientes(&e.conn, &f).unwrap();
    let segunda = recientes_desde(&e.conn, &f, 1).unwrap();
    assert_eq!(primera.total_sesiones, 2);
    assert_eq!(primera.resultados.len(), 1);
    assert_eq!(primera.resultados[0].id_publico, "cc-a1b2c3d4");
    assert_eq!(segunda.resultados[0].id_publico, "cx-cx01");
    assert_eq!(segunda.total_sesiones, 2);
    assert_eq!(primera.resultados[0].coincidencias, 0);
    assert!(!primera.resultados[0].fragmento.contains("ornitorrinco"));
    assert!(recientes_desde(&e.conn, &f, 9)
        .unwrap()
        .resultados
        .is_empty());
    let uso: i64 = e
        .conn
        .query_row("SELECT COUNT(*) FROM uso", [], |r| r.get(0))
        .unwrap();
    assert_eq!(uso, 0);
}

#[test]
fn recientes_respeta_fuente_proyecto_y_fecha() {
    use nisaba_core::{busqueda::recientes, modelo::Fuente};
    let e = indexado();
    let codex = recientes(
        &e.conn,
        &Filtros {
            fuente: Some(Fuente::Codex),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(codex.total_sesiones, 1);
    assert_eq!(codex.resultados[0].id_publico, "cx-cx01");
    let demo = recientes(
        &e.conn,
        &Filtros {
            proyecto: Some("Demo".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(demo.total_sesiones, 1);
    let futuro = recientes(
        &e.conn,
        &Filtros {
            desde: Some("2999-01-01".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(futuro.total_sesiones, 0);
}
