mod formato;
mod mcp;
mod web;

use clap::{Parser, Subcommand};
use formato::texto_busqueda;
use nisaba_core::busqueda::{
    buscar, buscar_sin_contar, mezclar, resolver, Filtros, Resolucion, EXCLUIDA,
};
use nisaba_core::indexador::{indexar, Progreso, ResultadoIndexado, Rutas};
use nisaba_core::lector::{a_markdown, leer, leer_version, Tramo};
use nisaba_core::modelo::{es_id_web, Fuente};
use nisaba_core::salud::{aviso_limpieza_claude, informe};
use nisaba_core::{db, exclusiones, exportar, importar};
use rusqlite::Connection;
use std::fs::OpenOptions;
use std::io::{ErrorKind, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "nisaba",
    version,
    about = "Tablilla local de tus sesiones con IA"
)]
struct Cli {
    #[command(subcommand)]
    orden: Orden,
}

#[derive(Subcommand)]
enum Orden {
    /// Busca en todas tus sesiones
    #[command(visible_alias = "search")]
    Buscar {
        consulta: String,
        /// claude-code | codex | claude-web | chatgpt (estas dos, con --web)
        #[arg(long, visible_alias = "source")]
        fuente: Option<String>,
        #[arg(long, visible_alias = "project")]
        proyecto: Option<String>,
        /// AAAA-MM-DD
        #[arg(long, visible_alias = "since")]
        desde: Option<String>,
        #[arg(long, visible_alias = "reasoning")]
        razonamiento: bool,
        #[arg(long, visible_alias = "code")]
        codigo: bool,
        #[arg(long, visible_alias = "limit", default_value_t = 10)]
        limite: usize,
        #[arg(long)]
        json: bool,
        /// Busca también en el archivo web; pide la contraseña
        #[arg(long)]
        web: bool,
    },
    /// Muestra una conversación en Markdown
    #[command(visible_alias = "show")]
    Ver {
        id: String,
        #[arg(long, visible_alias = "from")]
        desde: Option<usize>,
        #[arg(long, visible_alias = "count", default_value_t = 20)]
        cantidad: usize,
        #[arg(long, visible_alias = "all")]
        todo: bool,
        /// Versión anterior de una conversación del archivo web
        #[arg(long, value_name = "N")]
        version: Option<usize>,
    },
    /// Abre una conversación en la ventana de Nisaba
    #[command(visible_alias = "open")]
    Abrir { id: String },
    /// Guarda una conversación completa en Markdown
    #[command(visible_alias = "export")]
    Exportar {
        id: String,
        /// Incluye herramientas, razonamiento y eventos ocultos
        #[arg(long, visible_alias = "all")]
        todo: bool,
        /// Archivo de destino (por defecto, nombre propuesto en la carpeta actual)
        #[arg(long, visible_alias = "output")]
        salida: Option<PathBuf>,
        /// Exporta aunque haya posibles secretos
        #[arg(long, visible_alias = "force")]
        forzar: bool,
        /// Reemplaza el archivo de destino si ya existe
        #[arg(long, visible_alias = "overwrite")]
        sobrescribir: bool,
    },
    /// No indexar una carpeta (y sus subcarpetas)
    #[command(visible_alias = "exclude")]
    Excluir {
        #[arg(required_unless_present_any = ["quitar", "lista"], conflicts_with_all = ["quitar", "lista"])]
        carpeta: Option<String>,
        /// Borra también lo ya guardado de esa carpeta
        #[arg(long, visible_alias = "delete", requires = "carpeta")]
        borrar: bool,
        /// Vuelve a indexar una carpeta excluida
        #[arg(
            long,
            visible_alias = "remove",
            value_name = "CARPETA",
            conflicts_with = "lista"
        )]
        quitar: Option<String>,
        /// Lista las carpetas excluidas
        #[arg(long, visible_alias = "list")]
        lista: bool,
    },
    /// Indexa lo nuevo
    #[command(visible_alias = "index")]
    Indexar,
    /// Estado de salud de cada fuente
    #[command(visible_alias = "health")]
    Salud,
    /// Búsquedas por día y canal (últimos 14 días)
    #[command(visible_alias = "usage")]
    Uso,
    /// Guarda en el archivo web cifrado una exportación de Claude.ai o ChatGPT
    #[command(visible_alias = "import")]
    Importar { zip: PathBuf },
    /// Cambia la contraseña del archivo web
    #[command(visible_alias = "password")]
    Contrasena,
    /// Entra en el archivo web con el código de recuperación y pone contraseña nueva
    #[command(visible_alias = "recover")]
    Recuperar,
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
    db::abrir(&rutas.home.join("tablilla.db"))
        .map_err(|e| format!("no puedo abrir la tablilla: {e}"))
}

/// Indexa en silencio antes de consultar. `Ocupado` sigue; `Fallo` avisa y sigue.
fn indexar_antes(conn: &mut Connection, rutas: &Rutas) {
    if let ResultadoIndexado::Fallo(e) = indexar(conn, rutas, &mut |_p: Progreso| {}) {
        eprintln!("Error al indexar: {e}");
    }
}

/// Prepara la base en la que vive `id`: la local, al día, o el archivo web, ya desbloqueado.
/// `None` significa la local, que quien llama ya tiene abierta.
fn base_de(id: &str, conn: &mut Connection, rutas: &Rutas) -> Result<Option<Connection>, String> {
    if es_id_web(id) {
        return web::desbloquear(rutas).map(Some);
    }
    indexar_antes(conn, rutas);
    Ok(None)
}

fn ejecutar(orden: Orden) -> Result<u8, String> {
    nisaba_core::comprobar_entorno()?;
    let rutas = Rutas::desde_entorno();
    nisaba_core::preparar_home(&rutas.home)?;
    if let Orden::Mcp = orden {
        return mcp::servir(&rutas).map(|_| 0).map_err(|e| e.to_string());
    }
    let mut conn = abrir(&rutas)?;
    let e = |x: rusqlite::Error| x.to_string();
    match orden {
        Orden::Mcp => unreachable!("atendido arriba"),
        Orden::Buscar {
            consulta,
            fuente,
            proyecto,
            desde,
            razonamiento,
            codigo,
            limite,
            json,
            web,
        } => {
            let fuente =
                match fuente.as_deref() {
                    None => None,
                    Some(s) => Some(Fuente::desde_str(s).ok_or(
                        "fuente desconocida: usa claude-code, codex, claude-web o chatgpt",
                    )?),
                };
            let archivo = if web {
                web::desbloquear_si_hay(&rutas)?
            } else {
                None
            };
            indexar_antes(&mut conn, &rutas);
            let f = Filtros {
                fuente,
                proyecto,
                desde,
                razonamiento,
                codigo,
                limite,
            };
            let mut r = buscar(&conn, &consulta, &f, "cli").map_err(e)?;
            if let Some(archivo) = &archivo {
                let del_archivo = buscar_sin_contar(archivo, &consulta, &f).map_err(e)?;
                r = mezclar(r, del_archivo, f.limite);
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&r).map_err(|x| x.to_string())?
                );
                return Ok(0);
            }
            let tty = std::io::stdout().is_terminal();
            print!("{}", texto_busqueda(&r, tty, false));
            Ok(0)
        }
        Orden::Ver {
            id,
            desde,
            cantidad,
            todo,
            version,
        } => {
            if version.is_some() && !es_id_web(&id) {
                return Err(
                    "--version solo existe en el archivo web (ids cw-… y gpt-…).".to_string(),
                );
            }
            let archivo = base_de(&id, &mut conn, &rutas)?;
            let base = archivo.as_ref().unwrap_or(&conn);
            let cid = match resolver(base, &id).map_err(e)? {
                Resolucion::Uno(c) => c,
                Resolucion::Ninguno => {
                    eprintln!("No encuentro la conversación {id}");
                    return Ok(1);
                }
                Resolucion::Ambiguo(ids) => {
                    eprintln!("Id ambiguo: {id} encaja con {}", ids.join(", "));
                    return Ok(1);
                }
                Resolucion::Excluida => {
                    eprintln!("{EXCLUIDA}");
                    return Ok(1);
                }
            };
            let t = Tramo {
                desde,
                cantidad,
                alrededor_de: None,
                todo,
            };
            let l = match version {
                None => leer(base, cid, &t).map_err(e)?,
                Some(n) => {
                    let fila = web::fila_de_version(base, cid, &id, n)?;
                    let mut l = leer_version(base, fila, &t).map_err(e)?;
                    // `a_markdown` lo repite en el aviso de siguiente tramo, que debe seguir aquí.
                    l.id_publico = format!("{id} --version {n}");
                    l
                }
            };
            print!("{}", a_markdown(&l));
            Ok(0)
        }
        Orden::Abrir { id } => {
            indexar_antes(&mut conn, &rutas);
            match resolver(&conn, &id).map_err(e)? {
                Resolucion::Ninguno => return Err(format!("No encuentro la conversación {id}")),
                Resolucion::Ambiguo(ids) => {
                    return Err(format!("Id ambiguo: {id} encaja con {}", ids.join(", ")))
                }
                Resolucion::Excluida => return Err(EXCLUIDA.to_string()),
                Resolucion::Uno(_) => {}
            }
            let nombre = format!("nisaba-gui{}", std::env::consts::EXE_SUFFIX);
            let junto = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(&nombre)));
            let ejecutable = junto
                .filter(|p| p.is_file())
                .unwrap_or_else(|| PathBuf::from(&nombre));
            std::process::Command::new(ejecutable)
                .args(["--abrir", &id])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("No puedo abrir Nisaba: {e}. Instala nisaba-gui junto a nisaba o en el PATH."))?;
            Ok(0)
        }
        Orden::Exportar {
            id,
            todo,
            salida,
            forzar,
            sobrescribir,
        } => {
            let archivo = base_de(&id, &mut conn, &rutas)?;
            let base = archivo.as_ref().unwrap_or(&conn);
            let doc = exportar::documento(base, &id, todo)?;
            let secretos = exportar::posibles_secretos(&doc);
            if !secretos.is_empty() && !forzar {
                eprintln!("Posibles secretos en la conversación ({}):", secretos.len());
                for h in &secretos {
                    eprintln!("  {}: {}", h.tipo, h.muestra);
                }
                eprintln!("Revísalos; para exportar igualmente, usa --forzar.");
                return Ok(1);
            }
            let destino = match salida {
                Some(p) => p,
                None => PathBuf::from(exportar::nombre_propuesto(base, &id, "md")?),
            };
            // `create_new` lo comprueba y lo crea de una vez: entre mirar y escribir no cabe nadie.
            let mut opciones = OpenOptions::new();
            opciones
                .write(true)
                .create_new(!sobrescribir)
                .create(sobrescribir)
                .truncate(sobrescribir);
            opciones
                .open(&destino)
                .and_then(|mut f| f.write_all(doc.as_bytes()))
                .map_err(|x| match x.kind() {
                    ErrorKind::AlreadyExists => format!(
                        "{} ya existe; para reemplazarlo, usa --sobrescribir.",
                        destino.display()
                    ),
                    _ => format!("{}: {x}", destino.display()),
                })?;
            println!("Guardado en {}", destino.display());
            if archivo.is_some() {
                eprintln!("Este archivo queda sin cifrar.");
            }
            Ok(0)
        }
        Orden::Excluir {
            carpeta,
            borrar,
            quitar,
            lista,
        } => {
            // Solo se completan las rutas relativas (`.`, `foo`), sin resolver enlaces: la
            // carpeta puede no existir ya. Una ruta con raíz se guarda tal cual; en Windows,
            // `absolute` le añadiría la unidad y dejaría de casar con el `cwd` guardado.
            let absoluta = |c: &str| {
                if std::path::Path::new(c).has_root() {
                    return Ok(c.to_string());
                }
                std::path::absolute(c)
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|x| format!("{c}: {x}"))
            };
            if lista {
                for c in exclusiones::lista(&conn).map_err(e)? {
                    println!("{c}");
                }
            } else if let Some(c) = quitar {
                exclusiones::quitar(&conn, &absoluta(&c)?).map_err(e)?;
                println!("Volveré a indexar {c}.");
            } else if let Some(c) = carpeta {
                let c = absoluta(&c)?;
                let n = exclusiones::anadir(&conn, &rutas, &c, borrar).map_err(e)?;
                if borrar {
                    println!("Excluida {c}; borradas {n} conversaciones.");
                } else {
                    println!("Excluida {c}.");
                    if n > 0 {
                        eprintln!(
                            "Conservo {n} conversaciones de {c}; usa --borrar para eliminarlas."
                        );
                    }
                }
            }
            Ok(0)
        }
        Orden::Indexar => {
            let r = indexar(&mut conn, &rutas, &mut |p: Progreso| {
                eprintln!("Indexando {}/{}…", p.vistos, p.total);
            });
            match r {
                ResultadoIndexado::Hecho {
                    nuevos,
                    cambiados,
                    errores,
                } => {
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
                let marca = if f.vistos == f.parseados + f.excluidos {
                    "✓"
                } else {
                    "✗"
                };
                let ultima = f
                    .ultima
                    .as_deref()
                    .map(|u| u.chars().take(10).collect::<String>());
                let excluidos = if f.excluidos > 0 {
                    format!(" · excluidos {}", f.excluidos)
                } else {
                    String::new()
                };
                println!(
                    "{marca} {}  vistos {} · parseados {}{excluidos} · avisos {} · última {}",
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
            let settings = nisaba_core::ajustes_claude();
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
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })
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
        Orden::Importar { zip } => {
            // Antes de nada: un ZIP desconocido no pide contraseña ni deja archivo creado.
            importar::reconocer(&zip)?;
            let mut archivo = web::abrir_o_crear(&rutas)?;
            let r = importar::importar(&mut archivo, &zip)?;
            print!("{}", web::texto_importacion(&r, &zip));
            Ok(0)
        }
        Orden::Contrasena => {
            web::cambiar(&rutas)?;
            println!("Contraseña cambiada.");
            Ok(0)
        }
        Orden::Recuperar => {
            web::recuperar(&rutas)?;
            println!("Contraseña nueva guardada.");
            Ok(0)
        }
    }
}
