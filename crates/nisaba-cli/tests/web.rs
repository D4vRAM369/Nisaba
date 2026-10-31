//! Órdenes del archivo web. La bóveda y la tablilla se siembran desde `nisaba-core` porque
//! `FORMATOS` sigue vacío: ningún ZIP puede crearlas todavía a través de la CLI.

mod comun;
use comun::{ctx, Ctx};
use nisaba_core::archivo_web;
use nisaba_core::boveda::{self, ErrorBoveda, BOVEDA, TABLILLA_WEB};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Output, Stdio};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

const CONTRASENA: &str = "correcta caballo pila";
const MARCADOR: &str = "ZorroVioletaMarcador";
const ID: &str = "cw-web00001";

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn home(c: &Ctx) -> PathBuf {
    c.dir.path().join("home")
}

/// Sin entrada: una orden que intentase pedir un secreto se encontraría con el fin de archivo.
fn sin_entrada(c: &Ctx, args: &[&str]) -> Output {
    c.comando(args).stdin(Stdio::null()).output().unwrap()
}

fn con_entrada(c: &Ctx, args: &[&str], entrada: &str) -> Output {
    let mut hijo = c
        .comando(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    hijo.stdin
        .take()
        .unwrap()
        .write_all(entrada.as_bytes())
        .unwrap();
    hijo.wait_with_output().unwrap()
}

/// Una línea de stdin por respuesta.
fn lineas(respuestas: &[&str]) -> String {
    respuestas.iter().map(|r| format!("{r}\n")).collect()
}

fn insertar_conversacion(
    con: &rusqlite::Connection,
    fila: i64,
    id_original: &str,
    textos: &[&str],
) {
    con.execute(
        "INSERT INTO conversaciones(id, fuente, id_original, titulo, fin)
         VALUES (?1, 'claude-web', ?2, 'Plan de riego', '2026-10-01')",
        rusqlite::params![fila, id_original],
    )
    .unwrap();
    insertar_mensajes(con, fila, textos);
}

fn insertar_mensajes(con: &rusqlite::Connection, fila: i64, textos: &[&str]) {
    for (orden, texto) in textos.iter().enumerate() {
        let mensaje = fila * 100 + orden as i64;
        con.execute(
            "INSERT INTO mensajes(id, conversacion_id, orden, rol, oculto)
             VALUES (?1, ?2, ?3, 'usuario', 0)",
            rusqlite::params![mensaje, fila, orden as i64],
        )
        .unwrap();
        con.execute(
            "INSERT INTO partes(id, mensaje_id, orden, tipo, contenido)
             VALUES (?1, ?1, 0, 'texto', ?2)",
            rusqlite::params![mensaje, texto],
        )
        .unwrap();
    }
}

fn insertar_version(con: &rusqlite::Connection, fila: i64, sufijo: u32, textos: &[&str]) {
    con.execute(
        "INSERT INTO conversaciones(id, fuente, id_original, version_de, importada)
         VALUES (?1, 'claude-web', ?2, 1, '2026-10-02')",
        rusqlite::params![fila, format!("web00001abc@{sufijo}")],
    )
    .unwrap();
    insertar_mensajes(con, fila, textos);
}

/// Un archivo web de verdad con una conversación y dos versiones. Las filas van al revés que los
/// sufijos: el `n` de `--version` debe seguir el nombre, no la posición. Devuelve el código.
fn crear_archivo(c: &Ctx, contrasena: &str) -> String {
    fs::create_dir_all(home(c)).unwrap();
    let nueva = boveda::preparar(contrasena).unwrap();
    let codigo = nueva.codigo().to_string();
    let llave = nueva.guardar(&home(c)).unwrap();
    let con = archivo_web::abrir(&home(c), &llave).unwrap();
    insertar_conversacion(
        &con,
        1,
        "web00001abc",
        &[&format!("{MARCADOR} subtitulos vigente")],
    );
    insertar_version(&con, 2, 2, &["contenido del sufijo dos"]);
    insertar_version(
        &con,
        3,
        1,
        &["contenido del sufijo uno", "y una segunda parte"],
    );
    codigo
}

/// Archivos que no se pueden abrir: quien los toque falla y se nota.
fn archivo_ilegible(c: &Ctx) {
    fs::create_dir_all(home(c)).unwrap();
    fs::write(home(c).join(BOVEDA), "no soy una bóveda").unwrap();
    fs::write(home(c).join(TABLILLA_WEB), "no soy una tablilla").unwrap();
}

fn sin_rastro_web(c: &Ctx) {
    assert!(!home(c).join(BOVEDA).exists());
    assert!(!home(c).join(TABLILLA_WEB).exists());
}

#[test]
fn importar_zip_desconocido_falla_sin_crear_tablilla() {
    let c = ctx();
    let ruta = c.dir.path().join("otro.zip");
    let mut z = ZipWriter::new(fs::File::create(&ruta).unwrap());
    z.start_file("cualquier-cosa.json", SimpleFileOptions::default())
        .unwrap();
    z.write_all(b"[]").unwrap();
    z.finish().unwrap();

    let o = sin_entrada(&c, &["importar", ruta.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(err(&o).contains("formato desconocido"), "{}", err(&o));
    sin_rastro_web(&c);

    // El alias, y un archivo que ni siquiera es un ZIP.
    let falso = c.dir.path().join("falso.zip");
    fs::write(&falso, "no soy un zip").unwrap();
    let o = sin_entrada(&c, &["import", falso.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    sin_rastro_web(&c);
}

#[test]
fn sin_web_no_pide_contrasena() {
    let limpio = ctx();
    let c = ctx();
    archivo_ilegible(&c);
    for args in [&["buscar", "subtitulos"][..], &["ver", "cc-a1b2c3d4"]] {
        let o = sin_entrada(&c, args);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", err(&o));
        assert!(!err(&o).contains("ontraseña"), "{args:?}: {}", err(&o));
        assert_eq!(out(&o), out(&sin_entrada(&limpio, args)), "{args:?}");
    }
}

#[test]
fn buscar_web_sin_archivo_lo_explica() {
    let c = ctx();
    let sin_web = sin_entrada(&c, &["buscar", "subtitulos"]);
    let o = sin_entrada(&c, &["buscar", "subtitulos", "--web"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(
        err(&o).contains("Todavía no hay archivo web"),
        "{}",
        err(&o)
    );
    assert!(err(&o).contains("nisaba importar"), "{}", err(&o));
    assert_eq!(out(&o), out(&sin_web));
    sin_rastro_web(&c);
}

#[test]
fn buscar_web_mezcla_con_lo_local() {
    let c = ctx();
    crear_archivo(&c, CONTRASENA);

    let sin_web = sin_entrada(&c, &["buscar", "subtitulos"]);
    assert!(out(&sin_web).contains("cx-cx01"), "{}", out(&sin_web));
    assert!(!out(&sin_web).contains(ID), "{}", out(&sin_web));

    let entrada = lineas(&[CONTRASENA]);
    let o = con_entrada(&c, &["buscar", "subtitulos", "--web"], &entrada);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let s = out(&o);
    assert!(s.contains("cx-cx01"), "{s}");
    assert!(s.contains(ID), "{s}");
    assert!(s.contains("claude-web"), "{s}");
    assert!(!err(&o).contains(CONTRASENA), "{}", err(&o));

    let o = con_entrada(
        &c,
        &["buscar", "subtitulos", "--web", "--fuente", "claude-web"],
        &entrada,
    );
    assert!(out(&o).contains(ID), "{}", out(&o));
    assert!(!out(&o).contains("cx-cx01"), "{}", out(&o));

    // `--json` sigue siendo JSON limpio: la pregunta va por stderr.
    let o = con_entrada(&c, &["buscar", "subtitulos", "--web", "--json"], &entrada);
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["resultados"].as_array().unwrap().len(), 2);

    // Cada búsqueda suma una sola vez, aunque mire en dos bases.
    let o = sin_entrada(&c, &["uso"]);
    assert!(out(&o).contains(" 4\n"), "{}", out(&o));
}

#[test]
fn contrasena_incorrecta_sale_con_1() {
    let c = ctx();
    crear_archivo(&c, CONTRASENA);
    let mala = "esta contraseña no es";
    for args in [
        &["buscar", "subtitulos", "--web"][..],
        &["ver", ID],
        &["exportar", ID],
    ] {
        let o = con_entrada(&c, args, &lineas(&[mala]));
        assert_eq!(o.status.code(), Some(1), "{args:?}");
        assert_eq!(out(&o), "", "{args:?}");
        assert!(err(&o).contains("incorrectos"), "{args:?}: {}", err(&o));
        assert!(!err(&o).contains(mala), "{args:?}: {}", err(&o));
    }
}

#[test]
fn sin_respuesta_lo_dice_en_vez_de_probar_una_vacia() {
    let c = ctx();
    crear_archivo(&c, CONTRASENA);
    let o = sin_entrada(&c, &["buscar", "subtitulos", "--web"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("No llegó"), "{}", err(&o));
    assert!(!err(&o).contains("incorrectos"), "{}", err(&o));
}

#[test]
fn cambiar_y_recuperar() {
    let c = ctx();
    let codigo = crear_archivo(&c, CONTRASENA);
    let tablilla = fs::read(home(&c).join(TABLILLA_WEB)).unwrap();

    let nueva = "contraseña recién cambiada";
    let o = con_entrada(&c, &["contrasena"], &lineas(&[CONTRASENA, nueva, nueva]));
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(boveda::abrir(&home(&c), nueva).is_ok());
    assert_eq!(
        boveda::abrir(&home(&c), CONTRASENA).unwrap_err(),
        ErrorBoveda::Incorrecta
    );

    let otra = "contraseña tras perderla";
    let o = con_entrada(&c, &["recover"], &lineas(&[&codigo, otra, otra]));
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(boveda::abrir(&home(&c), otra).is_ok());
    assert_eq!(
        boveda::abrir(&home(&c), nueva).unwrap_err(),
        ErrorBoveda::Incorrecta
    );
    for secreto in [nueva, otra, codigo.as_str()] {
        assert!(!out(&o).contains(secreto));
        assert!(!err(&o).contains(secreto));
    }

    // Cambiar la llave de fuera no reescribe ni un byte de la tablilla.
    assert_eq!(fs::read(home(&c).join(TABLILLA_WEB)).unwrap(), tablilla);
    let o = con_entrada(&c, &["ver", ID], &lineas(&[otra]));
    assert!(out(&o).contains(MARCADOR), "{}", err(&o));
}

#[test]
fn repetir_mal_la_nueva_no_cambia_nada() {
    let c = ctx();
    let codigo = crear_archivo(&c, CONTRASENA);
    let antes = fs::read(home(&c).join(BOVEDA)).unwrap();
    let (una, otra) = ("contraseña larga número uno", "contraseña larga número dos");
    for (orden, primera) in [("password", CONTRASENA), ("recuperar", codigo.as_str())] {
        let o = con_entrada(&c, &[orden], &lineas(&[primera, una, otra]));
        assert_eq!(o.status.code(), Some(1), "{orden}");
        assert!(err(&o).contains("no coinciden"), "{}", err(&o));
        assert_eq!(fs::read(home(&c).join(BOVEDA)).unwrap(), antes);
    }
}

#[test]
fn contrasena_con_espacios_por_stdin_se_conserva() {
    let c = ctx();
    let con_espacios = "  ñandú con espacios  ";
    crear_archivo(&c, con_espacios);

    // `\r\n` es el final de línea de una tubería en Windows: se quita entero.
    for entrada in [format!("{con_espacios}\n"), format!("{con_espacios}\r\n")] {
        let o = con_entrada(&c, &["ver", ID], &entrada);
        assert_eq!(o.status.code(), Some(0), "{entrada:?}: {}", err(&o));
    }

    let recortada = lineas(&[con_espacios.trim()]);
    let o = con_entrada(&c, &["ver", ID], &recortada);
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn ver_web_lee_la_vigente_con_stdout_limpio() {
    let c = ctx();
    crear_archivo(&c, CONTRASENA);
    let o = con_entrada(&c, &["ver", ID], &lineas(&[CONTRASENA]));
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).starts_with("# Plan de riego"), "{}", out(&o));
    assert!(out(&o).contains(MARCADOR), "{}", out(&o));
    assert!(!out(&o).contains("sufijo"), "{}", out(&o));

    let o = con_entrada(&c, &["ver", "cw-noexiste"], &lineas(&[CONTRASENA]));
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("No encuentro"), "{}", err(&o));
}

#[test]
fn ver_version_sigue_el_nombre_y_no_la_posicion() {
    let c = ctx();
    crear_archivo(&c, CONTRASENA);
    let entrada = lineas(&[CONTRASENA]);

    let o = con_entrada(&c, &["ver", ID, "--version", "1"], &entrada);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).contains("contenido del sufijo uno"), "{}", out(&o));

    let o = con_entrada(&c, &["ver", ID, "--version", "2"], &entrada);
    assert!(out(&o).contains("contenido del sufijo dos"), "{}", out(&o));
}

#[test]
fn ver_version_ofrece_seguir_en_la_misma_version() {
    let c = ctx();
    crear_archivo(&c, CONTRASENA);
    let args = ["ver", ID, "--version", "1", "--cantidad", "1"];
    let o = con_entrada(&c, &args, &lineas(&[CONTRASENA]));
    let esperado = format!("nisaba ver {ID} --version 1 --desde 1");
    assert!(out(&o).contains(&esperado), "{}", out(&o));
}

#[test]
fn ver_version_desconocida_lo_dice() {
    let c = ctx();
    crear_archivo(&c, CONTRASENA);
    for n in ["0", "3"] {
        let o = con_entrada(&c, &["ver", ID, "--version", n], &lineas(&[CONTRASENA]));
        assert_eq!(o.status.code(), Some(1), "{n}");
        assert_eq!(out(&o), "", "{n}");
        let e = err(&o);
        assert!(e.contains(&format!("la versión {n}")), "{e}");
        assert!(e.contains(ID), "{e}");
    }
}

#[test]
fn ver_version_de_id_local_no_pide_contrasena() {
    let c = ctx();
    let o = sin_entrada(&c, &["ver", "cc-a1b2c3d4", "--version", "1"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("archivo web"), "{}", err(&o));
    assert!(!err(&o).contains("No llegó"), "{}", err(&o));
}

#[test]
fn exportar_web_avisa_por_stderr() {
    let c = ctx();
    crear_archivo(&c, CONTRASENA);
    let destino = c.dir.path().join("salida.md");
    let args = ["exportar", ID, "--salida", destino.to_str().unwrap()];
    let o = con_entrada(&c, &args, &lineas(&[CONTRASENA]));
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(err(&o).contains("Este archivo queda sin cifrar."));
    assert!(!out(&o).contains("sin cifrar"), "{}", out(&o));
    assert!(fs::read_to_string(&destino).unwrap().contains(MARCADOR));
}

#[test]
fn exportar_local_no_avisa_ni_pide_nada() {
    let c = ctx();
    let destino = c.dir.path().join("salida.md");
    let args = [
        "exportar",
        "cc-a1b2c3d4",
        "--salida",
        destino.to_str().unwrap(),
    ];
    let o = sin_entrada(&c, &args);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(!err(&o).contains("sin cifrar"), "{}", err(&o));
}

#[test]
fn sin_archivo_las_ordenes_propias_lo_dicen_sin_preguntar() {
    let c = ctx();
    for args in [&["contrasena"][..], &["recuperar"], &["ver", ID]] {
        let o = sin_entrada(&c, args);
        assert_eq!(o.status.code(), Some(1), "{args:?}");
        assert!(
            err(&o).contains("Todavía no hay archivo web"),
            "{}",
            err(&o)
        );
        assert!(!err(&o).contains("No llegó"), "{}", err(&o));
        sin_rastro_web(&c);
    }
}

#[test]
fn boveda_danada_muestra_su_mensaje_y_sale_con_1() {
    let c = ctx();
    crear_archivo(&c, CONTRASENA);

    fs::write(home(&c).join(BOVEDA), "no soy una bóveda").unwrap();
    let o = con_entrada(&c, &["ver", ID], &lineas(&[CONTRASENA]));
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("Problema con boveda.json"), "{}", err(&o));

    // Sin `boveda.json` ninguna contraseña serviría, así que ni se pregunta.
    fs::remove_file(home(&c).join(BOVEDA)).unwrap();
    let o = sin_entrada(&c, &["buscar", "subtitulos", "--web"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("no se puede recuperar"), "{}", err(&o));
    assert_eq!(out(&o), "");
}

#[test]
fn fuente_desconocida_nombra_todas() {
    let o = sin_entrada(&ctx(), &["buscar", "x", "--fuente", "gemini"]);
    assert_eq!(o.status.code(), Some(1));
    for fuente in ["claude-code", "codex", "claude-web", "chatgpt"] {
        assert!(err(&o).contains(fuente), "{}", err(&o));
    }
}
