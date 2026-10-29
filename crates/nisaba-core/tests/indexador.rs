use nisaba_core::indexador::ResultadoIndexado;
use nisaba_core::modelo::{Mensaje, Parte, Rol, TipoParte};
use nisaba_core::titulo::titulo_util;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
mod comun;
use comun::{correr, entorno, Entorno};
use nisaba_core::db;

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
    assert!(
        matches!(
            r,
            ResultadoIndexado::Hecho {
                nuevos: 3,
                errores: 1,
                ..
            }
        ),
        "{r:?}"
    );
    assert_eq!(contar(&e.conn, "SELECT count(*) FROM conversaciones"), 3);
    assert_eq!(
        contar(
            &e.conn,
            "SELECT count(*) FROM conversaciones WHERE fuente='codex'"
        ),
        1
    );
    assert_eq!(
        contar(
            &e.conn,
            "SELECT count(*) FROM archivos WHERE estado='error'"
        ),
        1
    );
    let ok: i64 = contar(&e.conn, "SELECT count(*) FROM archivos WHERE estado='ok'");
    assert_eq!(ok, 4); // principal, subagente, 2 codex
    let (padre, id_principal): (Option<i64>, i64) = (
        e.conn
            .query_row(
                "SELECT padre_id FROM conversaciones WHERE id_original='agent-zz'",
                [],
                |f| f.get(0),
            )
            .unwrap(),
        e.conn
            .query_row(
                "SELECT id FROM conversaciones WHERE id_original=?1",
                [PRINCIPAL],
                |f| f.get(0),
            )
            .unwrap(),
    );
    assert_eq!(padre, Some(id_principal));
    let proyecto: String = e
        .conn
        .query_row(
            "SELECT proyecto FROM conversaciones WHERE id_original=?1",
            [PRINCIPAL],
            |f| f.get(0),
        )
        .unwrap();
    assert_eq!(proyecto, "Demo");
    assert!(e
        .rutas
        .home
        .join(format!(
            "copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}.jsonl"
        ))
        .exists());
}

#[test]
fn segunda_pasada_no_reescribe() {
    let mut e = entorno();
    correr(&mut e);
    let r = correr(&mut e);
    assert!(
        matches!(
            r,
            ResultadoIndexado::Hecho {
                nuevos: 0,
                cambiados: 0,
                ..
            }
        ),
        "{r:?}"
    );
}

#[test]
fn archivo_crece_se_actualiza_con_id_estable() {
    let mut e = entorno();
    correr(&mut e);
    let sql = "SELECT id FROM conversaciones WHERE id_original=?1";
    let id1: i64 = e.conn.query_row(sql, [PRINCIPAL], |f| f.get(0)).unwrap();
    let n1 = contar(
        &e.conn,
        &format!("SELECT count(*) FROM mensajes WHERE conversacion_id={id1}"),
    );
    let ruta = ruta_principal(&e);
    let original = fs::read_to_string(&ruta).unwrap();
    // El fixture acaba en una línea cortada (escritura en curso): se descarta y se añade una completa.
    let mut s = original.trim_end_matches('\n')
        [..original.trim_end_matches('\n').rfind('\n').unwrap() + 1]
        .to_string();
    s.push_str(&format!("{{\"type\":\"user\",\"uuid\":\"u99\",\"parentUuid\":\"a2\",\"sessionId\":\"{PRINCIPAL}\",\"cwd\":\"/home/u/Proyectos/Demo\",\"timestamp\":\"2026-10-29T10:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"Una pregunta nueva sobre el esquema\"}}}}\n"));
    fs::write(&ruta, s).unwrap();
    let r = correr(&mut e);
    assert!(
        matches!(
            r,
            ResultadoIndexado::Hecho {
                nuevos: 0,
                cambiados: 1,
                ..
            }
        ),
        "{r:?}"
    );
    let id2: i64 = e.conn.query_row(sql, [PRINCIPAL], |f| f.get(0)).unwrap();
    assert_eq!(id1, id2);
    let n2 = contar(
        &e.conn,
        &format!("SELECT count(*) FROM mensajes WHERE conversacion_id={id2}"),
    );
    assert_eq!(n2, n1 + 1);
    assert_eq!(
        contar(
            &e.conn,
            "SELECT count(*) FROM partes_fts WHERE partes_fts MATCH 'esquema'"
        ),
        1
    );
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
        contar(
            &e.conn,
            &format!("SELECT count(*) FROM conversaciones WHERE id_original='{PRINCIPAL}'")
        ),
        1
    );
    assert!(e
        .rutas
        .home
        .join(format!(
            "copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}.jsonl"
        ))
        .exists());
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
    assert!(
        matches!(r, ResultadoIndexado::Hecho { errores: 1, .. }),
        "{r:?}"
    );
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
        partes: vec![Parte {
            tipo: TipoParte::Texto,
            contenido: texto.into(),
        }],
    }
}

#[test]
fn titulo_salta_genericos() {
    let ms = vec![
        usuario("continúa"),
        usuario("/compact"),
        usuario("Arregla el parseo de subtítulos en Mirlo"),
    ];
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
    assert!(
        matches!(r, ResultadoIndexado::Hecho { errores, .. } if errores >= 1),
        "{r:?}"
    );
    let estado: String = e
        .conn
        .query_row(
            "SELECT estado FROM archivos WHERE ruta=?1",
            [ruta_principal(&e).to_string_lossy()],
            |f| f.get(0),
        )
        .unwrap();
    assert_eq!(estado, "error");
    let msg: String = e
        .conn
        .query_row(
            "SELECT error FROM archivos WHERE ruta=?1",
            [ruta_principal(&e).to_string_lossy()],
            |f| f.get(0),
        )
        .unwrap();
    assert!(msg.starts_with("copia: "), "{msg}");
    fs::remove_file(e.rutas.home.join("copias/claude-code")).unwrap();
    correr(&mut e);
    assert!(e
        .rutas
        .home
        .join(format!(
            "copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}.jsonl"
        ))
        .exists());
    assert!(e
        .rutas
        .home
        .join(format!(
            "copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}/subagents/agent-zz.jsonl"
        ))
        .exists());
}

#[test]
fn fallo_si_home_es_un_archivo() {
    let mut e = entorno();
    let f = e._dir.path().join("archivo");
    fs::write(&f, "x").unwrap();
    e.rutas.home = f;
    assert!(matches!(correr(&mut e), ResultadoIndexado::Fallo(_)));
}

fn rollout(id: &str, texto: &str) -> String {
    format!(
        "{{\"timestamp\":\"2026-10-01T10:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"/home/u/P\"}}}}\n\
         {{\"timestamp\":\"2026-10-01T10:00:01.000Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{texto}\"}}]}}}}\n"
    )
}

fn entorno_vacio() -> Entorno {
    let mut e = entorno();
    std::fs::remove_dir_all(&e.rutas.claude).unwrap();
    std::fs::remove_dir_all(&e.rutas.codex).unwrap();
    std::fs::create_dir_all(&e.rutas.claude).unwrap();
    e.conn = db::abrir(&e.rutas.home.join("tablilla.db")).unwrap();
    e
}

#[test]
fn codex_jsonl_y_zst_de_la_misma_sesion_no_hacen_vaivén() {
    let mut e = entorno_vacio();
    let plano = rollout("mismo-id", "conversación inventada");
    let dir = e.rutas.codex.join("sessions/2026/10/01");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("rollout-2026-10-01T10-00-00-mismo-id.jsonl"),
        &plano,
    )
    .unwrap();
    let zst = ruzstd::encoding::compress_to_vec(
        plano.as_bytes(),
        ruzstd::encoding::CompressionLevel::Fastest,
    );
    std::fs::write(
        dir.join("rollout-2026-10-01T10-00-00-mismo-id.jsonl.zst"),
        zst,
    )
    .unwrap();

    let r1 = correr(&mut e);
    assert!(
        matches!(r1, ResultadoIndexado::Hecho { errores: 0, .. }),
        "{r1:?}"
    );
    let n: i64 = e
        .conn
        .query_row("SELECT count(*) FROM conversaciones", [], |f| f.get(0))
        .unwrap();
    assert_eq!(n, 1);
    match correr(&mut e) {
        ResultadoIndexado::Hecho {
            nuevos,
            cambiados,
            errores,
        } => {
            assert_eq!((nuevos, cambiados, errores), (0, 0, 0));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn zst_corrupto_es_error_en_archivos_sin_panico() {
    let mut e = entorno_vacio();
    let dir = e.rutas.codex.join("sessions/2026/10/01");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("rollout-2026-10-01T10-00-00-roto.jsonl.zst"),
        b"esto no es zstd",
    )
    .unwrap();
    match correr(&mut e) {
        ResultadoIndexado::Hecho { errores, .. } => assert_eq!(errores, 1),
        o => panic!("{o:?}"),
    }
    let (estado, err): (String, Option<String>) = e
        .conn
        .query_row("SELECT estado, error FROM archivos", [], |f| {
            Ok((f.get(0)?, f.get(1)?))
        })
        .unwrap();
    assert_eq!(estado, "error");
    assert!(err.unwrap().contains("zstd"));
}

fn n_mensajes(e: &Entorno) -> i64 {
    contar(
        &e.conn,
        &format!("SELECT count(*) FROM mensajes m JOIN conversaciones c ON c.id=m.conversacion_id WHERE c.id_original='{PRINCIPAL}'"),
    )
}

fn fijar_mtime(ruta: &std::path::Path, segs: u64) {
    let f = fs::OpenOptions::new().write(true).open(ruta).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(segs))
        .unwrap();
}

fn copia_principal(e: &Entorno) -> PathBuf {
    e.rutas.home.join(format!(
        "copias/claude-code/-home-u-Proyectos-Demo/{PRINCIPAL}.jsonl"
    ))
}

/// Trunca el original a sus 3 primeras líneas, con otra fecha de modificación.
fn truncar(e: &Entorno) -> String {
    let ruta = ruta_principal(e);
    let completo = fs::read_to_string(&ruta).unwrap();
    let corto: String = completo.lines().take(3).map(|l| format!("{l}\n")).collect();
    fs::write(&ruta, &corto).unwrap();
    fijar_mtime(&ruta, 1_900_000_000);
    corto
}

#[test]
fn codex_se_copia() {
    let mut e = entorno();
    correr(&mut e);
    let h = &e.rutas.home;
    assert!(h
        .join("copias/codex/sessions/2026/10/01/rollout-2026-10-01T10-00-00-cx01.jsonl")
        .exists());
    assert!(h
        .join("copias/codex/archived_sessions/rollout-2026-10-01T10-00-00-cx01.jsonl")
        .exists());
}

#[test]
fn original_encogido_no_pisa_copia() {
    let mut e = entorno();
    correr(&mut e);
    let copia = copia_principal(&e);
    let tam = fs::metadata(&copia).unwrap().len();
    let corto = truncar(&e);
    assert!((corto.len() as u64) < tam);
    correr(&mut e);
    assert_eq!(fs::metadata(&copia).unwrap().len(), tam);
    let dir = copia.parent().unwrap();
    let al_lado: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|f| {
            f.file_name()
                .to_string_lossy()
                .starts_with(&format!("{PRINCIPAL}.jsonl.encogido-"))
        })
        .collect();
    assert_eq!(al_lado.len(), 1);
    assert_eq!(fs::read_to_string(al_lado[0].path()).unwrap(), corto);
}

#[test]
fn conversacion_encogida_se_conserva() {
    let mut e = entorno();
    correr(&mut e);
    let antes = n_mensajes(&e);
    truncar(&e);
    let r = correr(&mut e);
    assert!(
        matches!(r, ResultadoIndexado::Hecho { errores: 1, .. }),
        "{r:?}"
    );
    assert_eq!(n_mensajes(&e), antes);
    let (estado, err): (String, String) = e
        .conn
        .query_row(
            "SELECT estado, error FROM archivos WHERE ruta=?1",
            [ruta_principal(&e).to_string_lossy()],
            |f| Ok((f.get(0)?, f.get(1)?)),
        )
        .unwrap();
    assert_eq!(estado, "error");
    assert!(err.starts_with("encogió: "), "{err}");
    assert!(err.contains("la versión anterior se conserva"), "{err}");
    match correr(&mut e) {
        ResultadoIndexado::Hecho {
            nuevos,
            cambiados,
            errores,
        } => assert_eq!((nuevos, cambiados, errores), (0, 0, 0)),
        o => panic!("{o:?}"),
    }
}

#[test]
fn vuelve_a_crecer_se_acepta() {
    let mut e = entorno();
    correr(&mut e);
    let antes = n_mensajes(&e);
    let ruta = ruta_principal(&e);
    let original = fs::read_to_string(&ruta).unwrap();
    truncar(&e);
    correr(&mut e);
    let sin_cortada = &original.trim_end_matches('\n')
        [..original.trim_end_matches('\n').rfind('\n').unwrap() + 1];
    let mut s = sin_cortada.to_string();
    s.push_str(&format!("{{\"type\":\"user\",\"uuid\":\"u99\",\"parentUuid\":\"a2\",\"sessionId\":\"{PRINCIPAL}\",\"cwd\":\"/home/u/Proyectos/Demo\",\"timestamp\":\"2026-10-29T10:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"Una pregunta nueva sobre el esquema\"}}}}\n"));
    fs::write(&ruta, s).unwrap();
    fijar_mtime(&ruta, 1_900_000_100);
    correr(&mut e);
    assert_eq!(n_mensajes(&e), antes + 1);
    let estado: String = e
        .conn
        .query_row(
            "SELECT estado FROM archivos WHERE ruta=?1",
            [ruta.to_string_lossy()],
            |f| f.get(0),
        )
        .unwrap();
    assert_eq!(estado, "ok");
}

#[test]
fn subagente_se_enlaza_si_padre_llega_en_otra_pasada() {
    for borrar_hijo in [false, true] {
        let mut e = entorno();
        let padre = ruta_principal(&e);
        let original = fs::read(&padre).unwrap();
        fs::remove_file(&padre).unwrap();
        correr(&mut e);
        let padre_id = |e: &Entorno| -> Option<i64> {
            e.conn
                .query_row(
                    "SELECT padre_id FROM conversaciones WHERE id_original='agent-zz'",
                    [],
                    |f| f.get(0),
                )
                .unwrap()
        };
        assert_eq!(padre_id(&e), None);
        let mensajes = contar(&e.conn, "SELECT count(*) FROM mensajes m JOIN conversaciones c ON c.id=m.conversacion_id WHERE c.id_original='agent-zz'");
        if borrar_hijo {
            fs::remove_file(e.rutas.claude.join(format!(
                "-home-u-Proyectos-Demo/{PRINCIPAL}/subagents/agent-zz.jsonl"
            )))
            .unwrap();
        }
        fs::write(&padre, original).unwrap();
        correr(&mut e);
        let esperado = e
            .conn
            .query_row(
                "SELECT id FROM conversaciones WHERE id_original=?1",
                [PRINCIPAL],
                |f| f.get(0),
            )
            .unwrap();
        assert_eq!(padre_id(&e), Some(esperado));
        assert_eq!(contar(&e.conn, "SELECT count(*) FROM mensajes m JOIN conversaciones c ON c.id=m.conversacion_id WHERE c.id_original='agent-zz'"), mensajes);
        assert_eq!(
            correr(&mut e),
            ResultadoIndexado::Hecho {
                nuevos: 0,
                cambiados: 0,
                errores: 0
            }
        );
    }
}

/// Deja la tablilla como la dejaba la v0.1.0: sin `cwd` y con `user_version = 0`.
fn volver_a_v0(e: &mut Entorno) {
    e.conn
        .execute_batch("ALTER TABLE conversaciones DROP COLUMN cwd; PRAGMA user_version = 0;")
        .unwrap();
    e.conn = db::abrir(&e.rutas.home.join("tablilla.db")).unwrap();
}

fn cwd_principal(e: &Entorno) -> Option<String> {
    e.conn
        .query_row(
            "SELECT cwd FROM conversaciones WHERE id_original=?1",
            [PRINCIPAL],
            |f| f.get(0),
        )
        .unwrap()
}

#[test]
fn migracion_rellena_cwd() {
    let mut e = entorno();
    correr(&mut e);
    volver_a_v0(&mut e);
    correr(&mut e);
    assert_eq!(cwd_principal(&e).as_deref(), Some("/home/u/Proyectos/Demo"));
}

#[test]
fn migracion_rellena_cwd_desde_copia() {
    let mut e = entorno();
    correr(&mut e);
    fs::remove_file(ruta_principal(&e)).unwrap();
    volver_a_v0(&mut e);
    correr(&mut e);
    assert_eq!(cwd_principal(&e).as_deref(), Some("/home/u/Proyectos/Demo"));
}

#[test]
fn migracion_conserva_conversacion_sin_fuente() {
    let mut e = entorno();
    correr(&mut e);
    let mensajes = contar(&e.conn, "SELECT count(*) FROM mensajes");
    let fts = contar(&e.conn, "SELECT count(*) FROM partes_fts");
    fs::remove_file(ruta_principal(&e)).unwrap();
    fs::remove_file(copia_principal(&e)).unwrap();
    volver_a_v0(&mut e);
    correr(&mut e);
    assert_eq!(contar(&e.conn, "SELECT count(*) FROM mensajes"), mensajes);
    assert_eq!(contar(&e.conn, "SELECT count(*) FROM partes_fts"), fts);
    assert_eq!(cwd_principal(&e), None);
}
