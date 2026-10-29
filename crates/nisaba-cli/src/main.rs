mod formato;
mod mcp;

use clap::{Parser, Subcommand};
use formato::texto_busqueda;
use nisaba_core::busqueda::{buscar, resolver_id, Filtros};
use nisaba_core::db;
use nisaba_core::indexador::{indexar, Progreso, ResultadoIndexado, Rutas};
use nisaba_core::lector::{a_markdown, leer, Tramo};
use nisaba_core::modelo::Fuente;
use nisaba_core::salud::{aviso_limpieza_claude, informe};
use rusqlite::Connection;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "nisaba", version, about = "Tablilla local de tus sesiones con IA")]
struct Cli {
    #[command(subcommand)]
    orden: Orden,
}

#[derive(Subcommand)]
enum Orden {
    /// Busca en todas tus sesiones
    Buscar {
        consulta: String,
        /// claude-code | codex
        #[arg(long)]
        fuente: Option<String>,
        #[arg(long)]
        proyecto: Option<String>,
        /// AAAA-MM-DD
        #[arg(long)]
        desde: Option<String>,
        #[arg(long)]
        razonamiento: bool,
        #[arg(long)]
        codigo: bool,
        #[arg(long, default_value_t = 10)]
        limite: usize,
        #[arg(long)]
        json: bool,
    },
    /// Muestra una conversación en Markdown
    Ver {
        id: String,
        #[arg(long)]
        desde: Option<usize>,
        #[arg(long, default_value_t = 20)]
        cantidad: usize,
        #[arg(long)]
        todo: bool,
    },
    /// Indexa lo nuevo
    Indexar,
    /// Estado de salud de cada fuente
    Salud,
    /// Búsquedas por día y canal (últimos 14 días)
    Uso,
    /// Servidor MCP por stdio (para Claude Code y otros agentes)
    Mcp,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match ejecutar(cli.orden) {
        Ok(c) => ExitCode::from(c),
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
    }
}

fn abrir(rutas: &Rutas) -> Result<Connection, String> {
    db::abrir(&rutas.home.join("tablilla.db")).map_err(|e| format!("no puedo abrir la tablilla: {e}"))
}

/// Indexa en silencio antes de consultar. `Ocupado` sigue; `Fallo` avisa y sigue.
fn indexar_antes(conn: &mut Connection, rutas: &Rutas) {
    if let ResultadoIndexado::Fallo(e) = indexar(conn, rutas, &mut |_p: Progreso| {}) {
        eprintln!("Error al indexar: {e}");
    }
}

fn ejecutar(orden: Orden) -> Result<u8, String> {
    let rutas = Rutas::desde_entorno();
    if let Orden::Mcp = orden {
        return mcp::servir(&rutas).map(|_| 0).map_err(|e| e.to_string());
    }
    let mut conn = abrir(&rutas)?;
    let e = |x: rusqlite::Error| x.to_string();
    match orden {
        Orden::Mcp => unreachable!("atendido arriba"),
        Orden::Buscar { consulta, fuente, proyecto, desde, razonamiento, codigo, limite, json } => {
            let fuente = match fuente.as_deref() {
                None => None,
                Some(s) => Some(Fuente::desde_str(s).ok_or("fuente desconocida: usa claude-code o codex")?),
            };
            indexar_antes(&mut conn, &rutas);
            let f = Filtros { fuente, proyecto, desde, razonamiento, codigo, limite, ..Default::default() };
            let r = buscar(&conn, &consulta, &f, "cli").map_err(e)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r).map_err(|x| x.to_string())?);
                return Ok(0);
            }
            let tty = std::io::stdout().is_terminal();
            print!("{}", texto_busqueda(&r, tty, false));
            Ok(0)
        }
        Orden::Ver { id, desde, cantidad, todo } => {
            indexar_antes(&mut conn, &rutas);
            let Some(cid) = resolver_id(&conn, &id).map_err(e)? else {
                eprintln!("No encuentro la conversación {id}");
                return Ok(1);
            };
            let t = Tramo { desde, cantidad, alrededor_de: None, todo };
            let l = leer(&conn, cid, &t).map_err(e)?;
            print!("{}", a_markdown(&l));
            Ok(0)
        }
        Orden::Indexar => {
            let r = indexar(&mut conn, &rutas, &mut |p: Progreso| {
                eprintln!("Indexando {}/{}…", p.vistos, p.total);
            });
            match r {
                ResultadoIndexado::Hecho { nuevos, cambiados, errores } => {
                    println!("Listo: {nuevos} nuevos, {cambiados} cambiados, {errores} con error.");
                    Ok(0)
                }
                ResultadoIndexado::Ocupado => {
                    println!("Ya hay una indexación en marcha.");
                    Ok(0)
                }
                ResultadoIndexado::Fallo(m) => {
                    eprintln!("Error al indexar: {m}");
                    Ok(1)
                }
            }
        }
        Orden::Salud => {
            indexar_antes(&mut conn, &rutas);
            for f in informe(&conn).map_err(e)? {
                let marca = if f.vistos == f.parseados { "✓" } else { "✗" };
                let ultima = f.ultima.as_deref().map(|u| u.chars().take(10).collect::<String>());
                println!(
                    "{marca} {}  vistos {} · parseados {} · avisos {} · última {}",
                    f.fuente.as_str(),
                    f.vistos,
                    f.parseados,
                    f.avisos,
                    ultima.as_deref().unwrap_or("—")
                );
                for (ruta, msg) in &f.errores {
                    println!("    {ruta}: {msg}");
                }
            }
            let settings = std::env::var_os("NISABA_CLAUDE_SETTINGS")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".claude/settings.json")
                });
            if let Some(a) = aviso_limpieza_claude(&settings) {
                println!("\n⚠ {a}");
            }
            Ok(0)
        }
        Orden::Uso => {
            let mut st = conn
                .prepare(
                    "SELECT dia, canal, busquedas FROM uso
                     WHERE dia >= date('now','localtime','-13 days') ORDER BY dia DESC, canal",
                )
                .map_err(e)?;
            let filas = st
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))
                .map_err(e)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(e)?;
            if filas.is_empty() {
                println!("Sin búsquedas en los últimos 14 días.");
            }
            for (dia, canal, n) in filas {
                println!("{dia}  {canal:<4} {n}");
            }
            Ok(0)
        }
    }
}
