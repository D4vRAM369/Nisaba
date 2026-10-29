use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn copiar(origen: &Path, destino: &Path) {
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

struct Ctx {
    dir: tempfile::TempDir,
}

fn ctx() -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("../nisaba-core/tests/fixtures");
    copiar(&fx.join("claude"), &dir.path().join("claude"));
    copiar(&fx.join("codex"), &dir.path().join("codex"));
    Ctx { dir }
}

impl Ctx {
    fn nisaba(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_nisaba"))
            .args(args)
            .env("NISABA_HOME", self.dir.path().join("home"))
            .env("NISABA_CLAUDE_DIR", self.dir.path().join("claude"))
            .env("NISABA_CODEX_DIR", self.dir.path().join("codex"))
            .env("NISABA_CLAUDE_SETTINGS", self.dir.path().join("settings.json"))
            .output()
            .unwrap()
    }
}

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
