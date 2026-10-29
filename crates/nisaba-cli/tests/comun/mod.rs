#![allow(dead_code)]
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

pub struct Ctx {
    pub dir: tempfile::TempDir,
}

pub fn ctx() -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("../nisaba-core/tests/fixtures");
    copiar(&fx.join("claude"), &dir.path().join("claude"));
    copiar(&fx.join("codex"), &dir.path().join("codex"));
    Ctx { dir }
}

impl Ctx {
    pub fn comando(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_nisaba"));
        c.args(args)
            .env("NISABA_HOME", self.dir.path().join("home"))
            .env("NISABA_CLAUDE_DIR", self.dir.path().join("claude"))
            .env("NISABA_CODEX_DIR", self.dir.path().join("codex"))
            .env(
                "NISABA_CLAUDE_SETTINGS",
                self.dir.path().join("settings.json"),
            );
        c
    }

    pub fn nisaba(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_nisaba"))
            .args(args)
            .env("NISABA_HOME", self.dir.path().join("home"))
            .env("NISABA_CLAUDE_DIR", self.dir.path().join("claude"))
            .env("NISABA_CODEX_DIR", self.dir.path().join("codex"))
            .env(
                "NISABA_CLAUDE_SETTINGS",
                self.dir.path().join("settings.json"),
            )
            .output()
            .unwrap()
    }
}
