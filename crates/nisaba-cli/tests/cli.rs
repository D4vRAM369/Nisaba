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
    use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command, time::{Duration, Instant}};
    let c = ctx();
    let bin = c.dir.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let cli = bin.join("nisaba");
    fs::copy(env!("CARGO_BIN_EXE_nisaba"), &cli).unwrap();
    let argumentos = c.dir.path().join("argumentos");
    let gui = bin.join("nisaba-gui");
    fs::write(&gui, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NISABA_TEST_ARGUMENTOS\"\n".replace("\\\"", "\"")).unwrap();
    fs::set_permissions(&gui, fs::Permissions::from_mode(0o755)).unwrap();
    let plantilla = c.comando(&[]);
    let mut comando = Command::new(&cli);
    comando.args(["abrir", "cx-cx01"]).envs(plantilla.get_envs().filter_map(|(k,v)| v.map(|v| (k,v))))
        .env("PATH", &bin).env("NISABA_TEST_ARGUMENTOS", &argumentos);
    let o = comando.output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let limite = Instant::now() + Duration::from_secs(2);
    while !Path::new(&argumentos).exists() && Instant::now() < limite { std::thread::sleep(Duration::from_millis(10)); }
    assert_eq!(fs::read_to_string(argumentos).unwrap(), "--abrir\ncx-cx01\n");
}
