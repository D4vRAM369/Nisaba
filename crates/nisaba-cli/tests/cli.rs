mod comun;
use comun::ctx;
use std::process::Output;

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn buscar_imprime_resultado() {
    let c = ctx();
    let o = c.nisaba(&["buscar", "subtitulos"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let s = out(&o);
    assert!(s.contains("cx-cx01"), "{s}");
    assert!(s.contains("Mirlo ·"), "{s}");
}

#[test]
fn buscar_json_valido() {
    let c = ctx();
    let o = c.nisaba(&["buscar", "subtitulos", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["resultados"].as_array().unwrap().len(), 1);
}

#[test]
fn ver_muestra_markdown() {
    let c = ctx();
    let o = c.nisaba(&["ver", "cx-cx01"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let s = out(&o);
    assert!(s.contains("### 👤 Tú"), "{s}");
    assert!(!s.contains("AGENTS.md"), "{s}");
}

#[test]
fn ver_id_desconocido() {
    let c = ctx();
    let o = c.nisaba(&["ver", "cc-zzzzzzzz"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("No encuentro"), "{}", err(&o));
}

#[test]
fn salud_marca_error() {
    let c = ctx();
    let o = c.nisaba(&["salud"]);
    let s = out(&o);
    assert!(s.contains("✗ claude-code"), "{s}");
    assert!(s.contains("roto.jsonl"), "{s}");
    assert!(s.contains("codex"), "{s}");
}

#[test]
fn indexar_y_uso() {
    let c = ctx();
    let o = c.nisaba(&["indexar"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    c.nisaba(&["buscar", "subtitulos"]);
    let o = c.nisaba(&["uso"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).contains("cli"), "{}", out(&o));
}

#[test]
fn abrir_id_desconocido() {
    let c = ctx();
    let o = c.nisaba(&["abrir", "cc-zzzzzzzz"]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(err(&o).contains("No encuentro"), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn abrir_lanza_gui_del_path_con_id_literal() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::Path,
        process::Command,
        time::{Duration, Instant},
    };
    let c = ctx();
    let bin = c.dir.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let cli = bin.join("nisaba");
    fs::copy(env!("CARGO_BIN_EXE_nisaba"), &cli).unwrap();
    let argumentos = c.dir.path().join("argumentos");
    let gui = bin.join("nisaba-gui");
    fs::write(
        &gui,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NISABA_TEST_ARGUMENTOS\"\n".replace("\\\"", "\""),
    )
    .unwrap();
    fs::set_permissions(&gui, fs::Permissions::from_mode(0o755)).unwrap();
    let plantilla = c.comando(&[]);
    let mut comando = Command::new(&cli);
    comando
        .args(["abrir", "cx-cx01"])
        .envs(plantilla.get_envs().filter_map(|(k, v)| v.map(|v| (k, v))))
        .env("PATH", &bin)
        .env("NISABA_TEST_ARGUMENTOS", &argumentos);
    let o = comando.output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let limite = Instant::now() + Duration::from_secs(2);
    while !Path::new(&argumentos).exists() && Instant::now() < limite {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::read_to_string(argumentos).unwrap(),
        "--abrir\ncx-cx01\n"
    );
}

#[cfg(unix)]
#[test]
fn datos_sin_permiso_avisa() {
    use std::os::unix::fs::PermissionsExt;
    let c = ctx();
    let bloqueada = c.dir.path().join("bloqueada");
    std::fs::create_dir_all(&bloqueada).unwrap();
    std::fs::set_permissions(&bloqueada, std::fs::Permissions::from_mode(0o555)).unwrap();
    let o = c
        .comando(&["salud"])
        .env("NISABA_HOME", bloqueada.join("nisaba"))
        .output()
        .unwrap();
    std::fs::set_permissions(&bloqueada, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(
        err(&o).contains("No puedo crear la carpeta de datos"),
        "{}",
        err(&o)
    );
}

const DEMO: &str = "/home/u/Proyectos/Demo";
const SECRETA: &str = "5ec0e700-0000-4000-8000-000000000003";

/// Añade una sesión de Claude Code con una clave de Anthropic dentro.
fn con_secreto(c: &comun::Ctx) {
    let dir = c.dir.path().join("claude/-home-u-Proyectos-Llaves");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{SECRETA}.jsonl")),
        format!("{{\"type\":\"user\",\"uuid\":\"s1\",\"parentUuid\":null,\"sessionId\":\"{SECRETA}\",\"cwd\":\"/home/u/Proyectos/Llaves\",\"timestamp\":\"2026-10-02T10:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"usa la clave sk-ant-api03-AbCdEfGhIjKlMnOpQra3F9 para probar\"}}}}\n"),
    )
    .unwrap();
}

#[test]
fn buscar_muestra_carpeta_y_json_incluye_cwd() {
    let c = ctx();
    let s = out(&c.nisaba(&["buscar", "subtitulos"]));
    assert!(s.contains("   /home/u/Proyectos/Mirlo"), "{s}");
    let o = c.nisaba(&["buscar", "subtitulos", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["resultados"][0]["cwd"], "/home/u/Proyectos/Mirlo");
    assert_eq!(v["resultados"][0]["id_original"], "cx01");
}

#[test]
fn ver_cabecera_con_carpeta_e_id_completo() {
    let c = ctx();
    let s = out(&c.nisaba(&["ver", "cc-a1b2c3d4"]));
    assert!(s.contains(DEMO), "{s}");
    assert!(s.contains("a1b2c3d4-0000-4000-8000-000000000001"), "{s}");
}

#[test]
fn exportar_escribe_markdown_completo() {
    let c = ctx();
    let destino = c.dir.path().join("fuera.md");
    let o = c.nisaba(&["exportar", "cx-cx01", "--salida", destino.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let md = std::fs::read_to_string(&destino).unwrap();
    assert!(md.starts_with("# "), "{md}");
    assert!(
        md.contains("/home/u/Proyectos/Mirlo") && md.contains("### 👤 Tú"),
        "{md}"
    );

    // Sin --salida: nombre propuesto en el directorio actual.
    let aqui = c.dir.path().join("aqui");
    std::fs::create_dir_all(&aqui).unwrap();
    let o = c
        .comando(&["exportar", "cx-cx01"])
        .current_dir(&aqui)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let nombres: Vec<String> = std::fs::read_dir(&aqui)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(nombres.len(), 1);
    assert!(
        nombres[0].starts_with("2026-10-01_mirlo_") && nombres[0].ends_with(".md"),
        "{nombres:?}"
    );
}

#[test]
fn exportar_no_pisa_un_archivo_existente() {
    let c = ctx();
    let destino = c.dir.path().join("mio.md");
    std::fs::write(&destino, "notas mías").unwrap();
    let ruta = destino.to_str().unwrap();

    let o = c.nisaba(&["exportar", "cx-cx01", "--salida", ruta]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("--sobrescribir"), "{}", err(&o));
    assert_eq!(std::fs::read_to_string(&destino).unwrap(), "notas mías");

    // `--forzar` acepta secretos, no pisar archivos: son dos decisiones distintas.
    let o = c.nisaba(&["exportar", "cx-cx01", "--salida", ruta, "--forzar"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(&destino).unwrap(), "notas mías");

    let o = c.nisaba(&["exportar", "cx-cx01", "--salida", ruta, "--sobrescribir"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(std::fs::read_to_string(&destino).unwrap().starts_with("# "));
}

#[test]
fn exportar_con_secreto_sin_forzar() {
    let c = ctx();
    con_secreto(&c);
    let destino = c.dir.path().join("s.md");
    let o = c.nisaba(&[
        "exportar",
        "cc-5ec0e700",
        "--salida",
        destino.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    let e = err(&o);
    assert!(e.contains("Anthropic") && e.contains("sk-ant…a3F9"), "{e}");
    assert!(e.contains("--forzar"), "{e}");
    assert!(!e.contains("AbCdEf"), "{e}");
    assert!(!destino.exists());
}

#[test]
fn exportar_con_secreto_forzado() {
    let c = ctx();
    con_secreto(&c);
    let destino = c.dir.path().join("s.md");
    let o = c.nisaba(&[
        "exportar",
        "cc-5ec0e700",
        "--salida",
        destino.to_str().unwrap(),
        "--forzar",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(destino.exists());
}

#[test]
fn excluir_lista_quitar() {
    let c = ctx();
    c.nisaba(&["indexar"]);
    assert_eq!(c.nisaba(&["excluir", DEMO]).status.code(), Some(0));
    assert!(out(&c.nisaba(&["excluir", "--lista"])).contains(DEMO));
    assert!(!out(&c.nisaba(&["buscar", "PillMate"])).contains("cc-a1b2c3d4"));
    let o = c.nisaba(&["ver", "cc-a1b2c3d4"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("excluida"), "{}", err(&o));
    assert!(out(&c.nisaba(&["salud"])).contains("excluidos 2"));

    assert_eq!(
        c.nisaba(&["excluir", "--quitar", DEMO]).status.code(),
        Some(0)
    );
    assert!(!out(&c.nisaba(&["excluir", "--lista"])).contains(DEMO));
    assert!(out(&c.nisaba(&["buscar", "PillMate"])).contains("cc-a1b2c3d4"));
}

#[test]
fn excluir_con_borrar() {
    let c = ctx();
    c.nisaba(&["indexar"]);
    let o = c.nisaba(&["excluir", DEMO]);
    assert!(err(&o).contains("Conservo 2 conversaciones"), "{}", err(&o));
    assert!(err(&o).contains("--borrar"), "{}", err(&o));
    let o = c.nisaba(&["excluir", DEMO, "--borrar"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let o = c.nisaba(&["ver", "cc-a1b2c3d4"]);
    assert!(err(&o).contains("No encuentro"), "{}", err(&o));
}

#[test]
fn alias_en_ingles_equivalen() {
    let c = ctx();
    con_secreto(&c);
    c.nisaba(&["indexar"]);
    let pares: &[(&[&str], &[&str])] = &[
        (
            &["buscar", "subtitulos", "--json"],
            &["search", "subtitulos", "--json"],
        ),
        (
            &[
                "buscar",
                "schedule",
                "--fuente",
                "claude-code",
                "--proyecto",
                "Demo",
                "--desde",
                "2026-01-01",
                "--razonamiento",
                "--codigo",
                "--limite",
                "3",
            ],
            &[
                "search",
                "schedule",
                "--source",
                "claude-code",
                "--project",
                "Demo",
                "--since",
                "2026-01-01",
                "--reasoning",
                "--code",
                "--limit",
                "3",
            ],
        ),
        (
            &[
                "ver",
                "cx-cx01",
                "--todo",
                "--desde",
                "1",
                "--cantidad",
                "2",
            ],
            &["show", "cx-cx01", "--all", "--from", "1", "--count", "2"],
        ),
        (&["salud"], &["health"]),
        (&["uso"], &["usage"]),
        (&["excluir", "--lista"], &["exclude", "--list"]),
    ];
    for (es, en) in pares {
        let (a, b) = (c.nisaba(es), c.nisaba(en));
        assert_eq!(a.status.code(), Some(0), "{es:?}: {}", err(&a));
        assert_eq!(out(&a), out(&b), "{es:?} ≠ {en:?}");
    }
    assert_eq!(c.nisaba(&["index"]).status.code(), Some(0));

    let (x, y) = (c.dir.path().join("x.md"), c.dir.path().join("y.md"));
    c.nisaba(&[
        "exportar",
        "cc-5ec0e700",
        "--todo",
        "--salida",
        x.to_str().unwrap(),
        "--forzar",
    ]);
    c.nisaba(&[
        "export",
        "cc-5ec0e700",
        "--all",
        "--output",
        y.to_str().unwrap(),
        "--force",
    ]);
    assert_eq!(std::fs::read(&x).unwrap(), std::fs::read(&y).unwrap());

    assert_eq!(
        c.nisaba(&["exclude", DEMO, "--delete"]).status.code(),
        Some(0)
    );
    assert_eq!(
        c.nisaba(&["exclude", "--remove", DEMO]).status.code(),
        Some(0)
    );
    assert!(out(&c.nisaba(&["--help"])).contains("search"));
}
