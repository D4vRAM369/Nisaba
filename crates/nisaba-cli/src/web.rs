//! Órdenes de la CLI sobre el archivo web: preguntar secretos y abrir, crear o reparar la bóveda.
//! Las preguntas van por stderr: stdout es de los resultados y debe poder ir a un archivo.

use nisaba_core::archivo_web;
use nisaba_core::boveda::{self, BOVEDA};
use nisaba_core::importar::Resumen;
use nisaba_core::indexador::Rutas;
use nisaba_core::lector::versiones;
use rusqlite::Connection;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use zeroize::Zeroizing;

const CONFIRMACION: &str = "guardado";

/// Reservada de antemano: al crecer, `String` dejaría en memoria una copia sin borrar.
const CAPACIDAD_LINEA: usize = 1024;

/// Hace una pregunta. Se inyecta para probar `crear` sin terminal.
type Preguntar<'a> = &'a mut dyn FnMut(&str) -> Result<Zeroizing<String>, String>;

/// Solo quita el final de línea: los espacios de una contraseña son parte de ella. Sin nada que
/// leer es un error, porque una respuesta vacía se confundiría con una contraseña incorrecta.
fn leer_linea(entrada: &mut dyn BufRead) -> Result<Zeroizing<String>, String> {
    let mut linea = Zeroizing::new(String::with_capacity(CAPACIDAD_LINEA));
    let leidos = entrada
        .read_line(&mut linea)
        .map_err(|e| format!("No pude leer la respuesta: {e}"))?;
    if leidos == 0 {
        return Err("No llegó ninguna respuesta por la entrada estándar.".to_string());
    }
    if linea.ends_with('\n') {
        linea.pop();
        if linea.ends_with('\r') {
            linea.pop();
        }
    }
    Ok(linea)
}

fn preguntar(pregunta: &str) -> Result<Zeroizing<String>, String> {
    eprint!("{pregunta}");
    leer_linea(&mut io::stdin().lock())
}

/// Sin eco en una terminal. Con `stdin` redirigido no hay eco que evitar y lee una línea.
fn leer_secreto(pregunta: &str) -> Result<Zeroizing<String>, String> {
    if !io::stdin().is_terminal() {
        return preguntar(pregunta);
    }
    rpassword::prompt_password(pregunta)
        .map(Zeroizing::new)
        .map_err(|e| format!("No pude leer de la terminal: {e}"))
}

fn pedir_nueva(secreto: Preguntar) -> Result<Zeroizing<String>, String> {
    let nueva = secreto("Contraseña nueva (mínimo 12 caracteres): ")?;
    if *secreto("Repítela: ")? != *nueva {
        return Err("Las contraseñas no coinciden.".to_string());
    }
    Ok(nueva)
}

fn hay_archivo(home: &Path) -> bool {
    home.join(BOVEDA).exists() || archivo_web::existe(home)
}

/// Sin `boveda.json` ninguna contraseña serviría, así que no se pregunta: `abrir` falla antes de
/// mirarla y explica el motivo.
fn exigir_boveda(home: &Path) -> Result<(), String> {
    if home.join(BOVEDA).exists() {
        return Ok(());
    }
    boveda::abrir(home, "").map(drop).map_err(|e| e.to_string())
}

fn abrir_archivo(home: &Path, llave: &boveda::LlaveMaestra) -> Result<Connection, String> {
    archivo_web::abrir(home, llave).map_err(|e| format!("no puedo abrir el archivo web: {e}"))
}

pub fn desbloquear(rutas: &Rutas) -> Result<Connection, String> {
    exigir_boveda(&rutas.home)?;
    let contrasena = leer_secreto("Contraseña del archivo web: ")?;
    let llave = boveda::abrir(&rutas.home, &contrasena).map_err(|e| e.to_string())?;
    abrir_archivo(&rutas.home, &llave)
}

/// `buscar --web` sin archivo lo explica y sigue con lo local, igual que sin `--web`.
pub fn desbloquear_si_hay(rutas: &Rutas) -> Result<Option<Connection>, String> {
    if hay_archivo(&rutas.home) {
        return desbloquear(rutas).map(Some);
    }
    eprintln!(
        "{} Usa `nisaba importar <zip>` para crearlo; sigo solo con lo local.",
        boveda::ErrorBoveda::NoExiste
    );
    Ok(None)
}

pub fn crear(rutas: &Rutas) -> Result<Connection, String> {
    crear_con(
        &rutas.home,
        &mut leer_secreto,
        &mut preguntar,
        &mut io::stdout(),
    )
}

/// `boveda.json` se escribe solo tras confirmar el código: antes de eso, cancelar no deja nada.
fn crear_con(
    home: &Path,
    secreto: Preguntar,
    linea: Preguntar,
    salida: &mut dyn Write,
) -> Result<Connection, String> {
    let contrasena = pedir_nueva(secreto)?;
    let nueva = boveda::preparar(&contrasena).map_err(|e| e.to_string())?;

    eprintln!("Código de recuperación. Se muestra solo esta vez:");
    writeln!(salida, "{}", nueva.codigo())
        .and_then(|()| salida.flush())
        .map_err(|e| format!("No pude mostrar el código: {e}. No se ha creado el archivo."))?;
    let pregunta = format!("Guárdalo y escribe «{CONFIRMACION}» para continuar: ");
    // Un error al teclear no debe tirar el código que ya se copió y dar otro.
    loop {
        let respuesta = linea(&pregunta).map_err(|e| format!("{e} No se ha creado el archivo."))?;
        if respuesta.trim() == CONFIRMACION {
            break;
        }
    }

    let llave = nueva.guardar(home).map_err(|e| e.to_string())?;
    abrir_archivo(home, &llave)
}

pub fn abrir_o_crear(rutas: &Rutas) -> Result<Connection, String> {
    if hay_archivo(&rutas.home) {
        desbloquear(rutas)
    } else {
        crear(rutas)
    }
}

pub fn recuperar(rutas: &Rutas) -> Result<(), String> {
    exigir_boveda(&rutas.home)?;
    let codigo = leer_secreto("Código de recuperación: ")?;
    let nueva = pedir_nueva(&mut leer_secreto)?;
    boveda::recuperar(&rutas.home, &codigo, &nueva)
        .map(drop)
        .map_err(|e| e.to_string())
}

pub fn cambiar(rutas: &Rutas) -> Result<(), String> {
    exigir_boveda(&rutas.home)?;
    let actual = leer_secreto("Contraseña actual: ")?;
    let nueva = pedir_nueva(&mut leer_secreto)?;
    boveda::cambiar_contrasena(&rutas.home, &actual, &nueva).map_err(|e| e.to_string())
}

fn id_original(conn: &Connection, fila: i64) -> Result<String, String> {
    conn.query_row(
        "SELECT id_original FROM conversaciones WHERE id = ?1",
        [fila],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

/// `n` es el del nombre `<id>@<n>`. No sirve la posición en `versiones`: esa lista va por orden de
/// fila, y `@n` por el orden en que cada contenido dejó de ser el vigente.
pub fn fila_de_version(
    conn: &Connection,
    conversacion: i64,
    id: &str,
    n: usize,
) -> Result<i64, String> {
    let nombre = format!("{}@{n}", id_original(conn, conversacion)?);
    let filas = versiones(conn, conversacion).map_err(|e| e.to_string())?;
    for v in &filas {
        if id_original(conn, v.id)? == nombre {
            return Ok(v.id);
        }
    }
    Err(format!(
        "{id} no tiene la versión {n}; tiene {} anteriores.",
        filas.len()
    ))
}

pub fn texto_importacion(r: &Resumen, zip: &Path) -> String {
    let mut s = format!(
        "Importado: {} nuevas, {} actualizadas, {} con versión nueva, {} sin cambios.\n",
        r.nuevas, r.actualizadas, r.versionadas, r.iguales
    );
    for aviso in &r.avisos {
        s.push_str(&format!("Aviso: {aviso}\n"));
    }
    s.push_str(&format!(
        "Tu ZIP sigue sin cifrar en {}. Nisaba no lo toca; guárdalo en un lugar cifrado.\n",
        zip.display()
    ));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use nisaba_core::boveda::TABLILLA_WEB;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use tempfile::tempdir;

    const CONTRASENA: &str = "correcta caballo pila";

    /// Responde lo previsto, una vez cada cosa; agotado, falla como un `stdin` terminado.
    fn guion(respuestas: &[&str]) -> impl FnMut(&str) -> Result<Zeroizing<String>, String> + use<> {
        let mut quedan: VecDeque<String> = respuestas.iter().map(|r| r.to_string()).collect();
        move |_| {
            quedan
                .pop_front()
                .map(Zeroizing::new)
                .ok_or_else(|| "No llegó ninguna respuesta.".to_string())
        }
    }

    fn crear_con_guion(
        home: &Path,
        secretos: &[&str],
        lineas: &[&str],
        salida: &mut Vec<u8>,
    ) -> Result<Connection, String> {
        crear_con(home, &mut guion(secretos), &mut guion(lineas), salida)
    }

    fn vacia(home: &Path) -> bool {
        std::fs::read_dir(home).unwrap().next().is_none()
    }

    #[test]
    fn leer_linea_quita_solo_el_final_de_linea() {
        for (entrada, esperado) in [
            ("uno\n", "uno"),
            ("uno\r\n", "uno"),
            ("  uno  \n", "  uno  "),
            ("\tuno\t\r\n", "\tuno\t"),
            ("sin final", "sin final"),
            ("con\rretorno\n", "con\rretorno"),
            ("\r\r\n", "\r"),
            ("\n", ""),
            ("primera\nsegunda\n", "primera"),
        ] {
            let leida = leer_linea(&mut entrada.as_bytes()).unwrap();
            assert_eq!(leida.as_str(), esperado, "{entrada:?}");
        }
    }

    #[test]
    fn leer_linea_sin_nada_es_error() {
        assert!(leer_linea(&mut "".as_bytes())
            .unwrap_err()
            .contains("No llegó"));
    }

    #[test]
    fn dos_lecturas_seguidas_toman_dos_lineas() {
        let mut entrada = "uno\ndos\n".as_bytes();
        assert_eq!(leer_linea(&mut entrada).unwrap().as_str(), "uno");
        assert_eq!(leer_linea(&mut entrada).unwrap().as_str(), "dos");
    }

    #[test]
    fn crear_sin_confirmar_no_escribe() {
        let d = tempdir().unwrap();
        let mut salida = Vec::new();
        let secretos = [CONTRASENA, CONTRASENA];
        let e = crear_con_guion(d.path(), &secretos, &["no"], &mut salida)
            .err()
            .unwrap();
        assert!(e.contains("No se ha creado"), "{e}");
        // El código se llegó a mostrar, pero la bóveda no existe: cancelar es seguro.
        assert!(!salida.is_empty());
        assert!(vacia(d.path()));
    }

    #[test]
    fn crear_exige_la_palabra_y_deja_todo_listo() {
        let d = tempdir().unwrap();
        let mut salida = Vec::new();
        let secretos = [CONTRASENA, CONTRASENA];
        let lineas = ["vale", " guardado \n"];
        let con = crear_con_guion(d.path(), &secretos, &lineas, &mut salida).unwrap();
        drop(con);

        assert!(d.path().join(BOVEDA).exists());
        assert!(d.path().join(TABLILLA_WEB).exists());
        let llave = boveda::abrir(d.path(), CONTRASENA).unwrap();
        // Lo que salió por stdout es el código, y solo el código.
        let codigo = String::from_utf8(salida).unwrap();
        let recuperada = boveda::recuperar(d.path(), &codigo, "contraseña tras perderla").unwrap();
        assert_eq!(*recuperada.hex(), *llave.hex());
        assert!(!codigo.contains(CONTRASENA));
    }

    #[test]
    fn crear_con_contrasenas_distintas_no_escribe_ni_muestra_codigo() {
        let d = tempdir().unwrap();
        let mut salida = Vec::new();
        let secretos = [CONTRASENA, "otra contraseña larga"];
        let e = crear_con_guion(d.path(), &secretos, &["guardado"], &mut salida)
            .err()
            .unwrap();
        assert!(e.contains("no coinciden"), "{e}");
        assert!(salida.is_empty());
        assert!(vacia(d.path()));
    }

    #[test]
    fn crear_con_contrasena_corta_no_escribe_ni_muestra_codigo() {
        let d = tempdir().unwrap();
        let mut salida = Vec::new();
        let secretos = ["corta", "corta"];
        let e = crear_con_guion(d.path(), &secretos, &["guardado"], &mut salida)
            .err()
            .unwrap();
        assert!(e.contains("al menos 12"), "{e}");
        assert!(salida.is_empty());
        assert!(vacia(d.path()));
    }

    #[test]
    fn crear_no_pisa_una_tablilla_que_aparezca_entre_medias() {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join(TABLILLA_WEB), "de otro proceso").unwrap();
        let mut salida = Vec::new();
        let secretos = [CONTRASENA, CONTRASENA];
        let e = crear_con_guion(d.path(), &secretos, &["guardado"], &mut salida)
            .err()
            .unwrap();
        assert!(e.contains("Ya hay un archivo web"), "{e}");
        assert!(!d.path().join(BOVEDA).exists());
    }

    #[test]
    fn el_resumen_lleva_contadores_avisos_y_el_recordatorio() {
        let r = Resumen {
            nuevas: 1,
            actualizadas: 2,
            versionadas: 3,
            iguales: 4,
            avisos: vec!["parte sin leer".to_string()],
        };
        let s = texto_importacion(&r, &PathBuf::from("/tmp/exportacion.zip"));
        assert!(
            s.contains("1 nuevas, 2 actualizadas, 3 con versión nueva, 4 sin cambios"),
            "{s}"
        );
        assert!(s.contains("parte sin leer"), "{s}");
        assert!(
            s.ends_with(
                "Tu ZIP sigue sin cifrar en /tmp/exportacion.zip. Nisaba no lo toca; guárdalo en un lugar cifrado.\n"
            ),
            "{s}"
        );
    }
}
