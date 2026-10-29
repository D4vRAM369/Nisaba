mod comun;

use comun::{copiar, correr, entorno, Entorno};
use nisaba_core::busqueda::{buscar, resolver_id, Filtros};
use nisaba_core::db;
use nisaba_core::indexador::{indexar, Rutas};
use nisaba_core::lector::{a_markdown, leer, Bloque, Tramo};
use std::path::Path;

fn indexado() -> Entorno {
    let mut e = entorno();
    correr(&mut e);
    e
}

fn id_de(e: &Entorno, publico: &str) -> i64 {
    resolver_id(&e.conn, publico).unwrap().unwrap()
}

#[test]
fn solo_conversacion_sin_ruido() {
    let e = indexado();
    let l = leer(&e.conn, id_de(&e, "cc-a1b2c3d4"), &Tramo::default()).unwrap();
    assert!(!l
        .bloques
        .iter()
        .any(|b| matches!(b, Bloque::Sistema { .. })));
    assert!(
        matches!(
            l.bloques.as_slice(),
            [
                Bloque::Usuario { .. },
                Bloque::Asistente { .. },
                Bloque::Herramientas { n: 2, archivos: 1 },
                Bloque::Asistente { .. },
                Bloque::Subagente { .. }
            ]
        ),
        "{:#?}",
        l.bloques
    );
    assert_eq!(l.total, 5);
    assert_eq!(l.id_publico, "cc-a1b2c3d4");
}

#[test]
fn subagente_en_su_sitio() {
    let e = indexado();
    let l = leer(&e.conn, id_de(&e, "cc-a1b2c3d4"), &Tramo::default()).unwrap();
    assert!(l.bloques.iter().any(|b| matches!(b,
        Bloque::Subagente { descripcion, .. } if descripcion == "Revisión final")));
}

#[test]
fn todo_muestra_ocultos() {
    let e = indexado();
    let t = Tramo {
        todo: true,
        cantidad: 100,
        ..Default::default()
    };
    let l = leer(&e.conn, id_de(&e, "cc-a1b2c3d4"), &t).unwrap();
    assert!(l.bloques.iter().any(|b| matches!(b,
        Bloque::Sistema { texto } if texto.contains("<system-reminder>"))));
}

#[test]
fn rama_activa() {
    let dir = tempfile::tempdir().unwrap();
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rama");
    let claude = dir.path().join("claude");
    let codex = dir.path().join("codex");
    copiar(&fx, &claude);
    std::fs::create_dir_all(&codex).unwrap();
    let home = dir.path().join("home");
    let mut conn = db::abrir(&home.join("tablilla.db")).unwrap();
    indexar(
        &mut conn,
        &Rutas {
            home,
            claude,
            codex,
        },
        &mut |_| {},
    );

    let id = resolver_id(&conn, "cc-b1b1b1b1").unwrap().unwrap();
    let l = leer(
        &conn,
        id,
        &Tramo {
            cantidad: 100,
            ..Default::default()
        },
    )
    .unwrap();
    let md = a_markdown(&l);
    assert!(!md.contains("ramaperdida"), "{md}");
    assert!(md.contains("Mejor usemos JSON."));
    assert_eq!(l.total, 4);
    let r = buscar(&conn, "ramaperdida", &Filtros::default(), "cli").unwrap();
    assert_eq!(r.resultados.len(), 1);
}

/// Conversación sintética: 60 mensajes de texto, sin uuid.
fn sintetica() -> (Entorno, i64) {
    let e = entorno();
    e.conn.execute(
        "INSERT INTO conversaciones(fuente, id_original, titulo) VALUES ('claude-code','sintetica-0001','Larga')",
        [],
    ).unwrap();
    let cid = e.conn.last_insert_rowid();
    for i in 0..60 {
        let rol = if i % 2 == 0 { "usuario" } else { "asistente" };
        e.conn
            .execute(
                "INSERT INTO mensajes(conversacion_id, orden, rol, oculto) VALUES (?1, ?2, ?3, 0)",
                rusqlite::params![cid, i, rol],
            )
            .unwrap();
        let mid = e.conn.last_insert_rowid();
        e.conn.execute(
            "INSERT INTO partes(mensaje_id, orden, tipo, contenido) VALUES (?1, 0, 'texto', ?2)",
            rusqlite::params![mid, format!("mensaje {i}")],
        ).unwrap();
    }
    (e, cid)
}

#[test]
fn tramo_centrado() {
    let (e, cid) = sintetica();
    let t = Tramo {
        alrededor_de: Some(40),
        cantidad: 20,
        ..Default::default()
    };
    let l = leer(&e.conn, cid, &t).unwrap();
    assert_eq!((l.desde, l.bloques.len(), l.total), (30, 20, 60));
    assert_eq!(l.coincidencia, Some(40));
}

#[test]
fn markdown_indica_siguiente_tramo() {
    let (e, cid) = sintetica();
    let t = Tramo {
        alrededor_de: Some(40),
        cantidad: 20,
        ..Default::default()
    };
    let md = a_markdown(&leer(&e.conn, cid, &t).unwrap());
    assert!(md.contains("bloques 31–50 de 60"), "{md}");
    assert!(md.trim_end().ends_with("--desde 50"), "{md}");
    assert!(md.contains("nisaba ver cc-sintetic --desde 50"), "{md}");
}

#[test]
fn rama_atraviesa_attachment() {
    let e = indexado();
    let l = leer(&e.conn, id_de(&e, "cc-a1b2c3d4"), &Tramo::default()).unwrap();
    assert!(
        matches!(l.bloques.first(), Some(Bloque::Usuario { .. })),
        "{:#?}",
        l.bloques
    );
    assert_eq!(l.total, 5);
}

#[test]
fn padre_ausente_no_trunca() {
    let e = entorno();
    e.conn.execute(
        "INSERT INTO conversaciones(fuente, id_original, titulo, hoja) VALUES ('claude-code','rota-00001','Rota','m4')",
        [],
    ).unwrap();
    let cid = e.conn.last_insert_rowid();
    // m3 apunta a "fantasma", que no está indexado.
    for (i, (uuid, padre)) in [
        ("m1", None),
        ("m2", Some("m1")),
        ("m3", Some("fantasma")),
        ("m4", Some("m3")),
    ]
    .into_iter()
    .enumerate()
    {
        e.conn.execute(
            "INSERT INTO mensajes(conversacion_id, orden, uuid, id_padre, rol, oculto) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            rusqlite::params![cid, i as i64, uuid, padre, if i % 2 == 0 { "usuario" } else { "asistente" }],
        ).unwrap();
        let mid = e.conn.last_insert_rowid();
        e.conn.execute(
            "INSERT INTO partes(mensaje_id, orden, tipo, contenido) VALUES (?1, 0, 'texto', ?2)",
            rusqlite::params![mid, format!("texto {uuid}")],
        ).unwrap();
    }
    let l = leer(&e.conn, cid, &Tramo::default()).unwrap();
    assert_eq!(l.total, 4, "{:#?}", l.bloques);
}

#[test]
fn rama_cruza_la_compactacion() {
    // Tras /compact, `compact_boundary` tiene parentUuid null pero logicalParentUuid
    // apunta al último mensaje anterior: la rama debe incluir lo previo.
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join("claude");
    let codex = dir.path().join("codex");
    std::fs::create_dir_all(claude.join("-home-u-P")).unwrap();
    std::fs::create_dir_all(&codex).unwrap();
    let l = |t: &str, u: &str, p: &str, extra: &str, c: &str| {
        format!(
            r#"{{"type":"{t}","uuid":"{u}","parentUuid":{p},"sessionId":"s",{extra}"message":{{"role":"{t}","content":"{c}"}}}}"#
        )
    };
    let lineas = [
        l("user", "u1", "null", "", "pregunta anterior a compactar"),
        l("assistant", "a1", "\"u1\"", "", "respuesta anterior a compactar"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"cb","parentUuid":null,"logicalParentUuid":"a1","sessionId":"s","content":"Conversation compacted"}"#.to_string(),
        l("user", "u2", "\"cb\"", "", "pregunta posterior"),
        l("assistant", "a2", "\"u2\"", "", "respuesta posterior"),
        r#"{"type":"last-prompt","lastPrompt":"x","leafUuid":"a2","sessionId":"s"}"#.to_string(),
    ];
    std::fs::write(
        claude.join("-home-u-P/zzzz0001.jsonl"),
        lineas.join("\n") + "\n",
    )
    .unwrap();
    let home = dir.path().join("home");
    let mut conn = db::abrir(&home.join("tablilla.db")).unwrap();
    indexar(
        &mut conn,
        &Rutas {
            home,
            claude,
            codex,
        },
        &mut |_| {},
    );
    let id = resolver_id(&conn, "cc-zzzz0001").unwrap().unwrap();
    let md = a_markdown(
        &leer(
            &conn,
            id,
            &Tramo {
                cantidad: 100,
                ..Default::default()
            },
        )
        .unwrap(),
    );
    assert!(md.contains("respuesta anterior a compactar"), "{md}");
    assert!(md.contains("respuesta posterior"), "{md}");
}

#[test]
fn todo_permite_leer_razonamiento_y_herramientas() {
    let e = entorno();
    e.conn.execute(
        "INSERT INTO conversaciones(fuente, id_original, titulo) VALUES ('codex','detalle-0001','Detalle')",
        [],
    ).unwrap();
    let cid = e.conn.last_insert_rowid();
    e.conn.execute(
        "INSERT INTO mensajes(conversacion_id, orden, rol, oculto) VALUES (?1, 0, 'asistente', 0)",
        [cid],
    ).unwrap();
    let mid = e.conn.last_insert_rowid();
    let razonamiento = "Comparo las rutas\ny elijo la más corta.";
    let herramienta = "Read: {\"file_path\":\"/tmp/inventado.rs\"}";
    for (orden, tipo, contenido) in [
        (0, "razonamiento", razonamiento),
        (1, "herramienta", herramienta),
    ] {
        e.conn
            .execute(
                "INSERT INTO partes(mensaje_id, orden, tipo, contenido) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![mid, orden, tipo, contenido],
            )
            .unwrap();
    }
    let normal = a_markdown(&leer(&e.conn, cid, &Tramo::default()).unwrap());
    assert!(!normal.contains(razonamiento));
    assert!(!normal.contains(herramienta));
    assert!(normal.contains("1 herramientas · 1 archivos"));
    let todo = a_markdown(
        &leer(
            &e.conn,
            cid,
            &Tramo {
                todo: true,
                ..Default::default()
            },
        )
        .unwrap(),
    );
    assert!(todo.contains(razonamiento), "{todo}");
    assert!(todo.contains(herramienta), "{todo}");
}
