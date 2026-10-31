//! Importación de exportaciones web a la tablilla cifrada.

pub mod fusion;
mod zip;

use std::collections::HashSet;
use std::path::Path;

use rusqlite::{Connection, ErrorCode, TransactionBehavior};

use self::fusion::{fusionar, Efecto};
use self::zip::{Lector, LIMITE_TOTAL};
use crate::modelo::ConversacionImportada;

/// Lo que dejó una importación ya confirmada.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resumen {
    pub nuevas: usize,
    pub actualizadas: usize,
    pub versionadas: usize,
    pub iguales: usize,
    pub avisos: Vec<String>,
}

/// Por donde un formato entrega cada conversación.
pub(crate) type Salida<'a> = &'a mut dyn FnMut(ConversacionImportada) -> Result<(), String>;

/// Un origen de exportación: cómo reconocerlo por sus nombres y cómo leerlo. `leer` entrega cada
/// conversación según sale, sin acumularlas.
pub(crate) struct Formato {
    pub nombre: &'static str,
    pub reconoce: fn(&[String]) -> bool,
    pub leer: fn(&mut Lector, Salida<'_>, &mut Vec<String>) -> Result<(), String>,
}

pub(crate) const FORMATOS: &[Formato] = &[];

pub fn importar(conn: &mut Connection, zip: &Path) -> Result<Resumen, String> {
    importar_con(conn, zip, FORMATOS)
}

/// Comprueba el formato sin escribir: la CLI y la GUI lo llaman antes de crear el archivo.
pub fn reconocer(zip: &Path) -> Result<&'static str, String> {
    reconocer_con(zip, FORMATOS)
}

fn reconocer_con(zip: &Path, formatos: &[Formato]) -> Result<&'static str, String> {
    let lector = Lector::abrir(zip, LIMITE_TOTAL)?;
    Ok(detectar(&lector, formatos)?.nombre)
}

/// El mensaje es fijo: los nombres de un ZIP desconocido son contenido del usuario.
fn detectar<'a>(lector: &Lector, formatos: &'a [Formato]) -> Result<&'a Formato, String> {
    let nombres = lector.nombres();
    formatos
        .iter()
        .find(|f| (f.reconoce)(&nombres))
        .ok_or_else(|| {
            "formato desconocido: no es una exportación de Claude.ai ni de ChatGPT".to_string()
        })
}

pub(crate) fn importar_con(
    conn: &mut Connection,
    zip: &Path,
    formatos: &[Formato],
) -> Result<Resumen, String> {
    importar_hasta(conn, zip, formatos, LIMITE_TOTAL)
}

/// Cada conversación se fusiona al salir del lector, sin acumularlas en memoria, dentro de una
/// sola transacción que solo se confirma al acabar: si algo falla, soltarla lo deshace todo.
/// `Immediate` toma la escritura desde el principio: otra importación espera aquí, hasta
/// `busy_timeout`, en lugar de chocar a mitad, cuando ya no podría esperar porque partió de una
/// foto vieja de la base.
fn importar_hasta(
    conn: &mut Connection,
    zip: &Path,
    formatos: &[Formato],
    limite: u64,
) -> Result<Resumen, String> {
    debug_assert_eq!(
        conn.query_row("PRAGMA temp_store", [], |f| f.get::<_, i64>(0)),
        Ok(2),
        "temp_store: la conexión debe guardar los temporales en memoria"
    );
    let mut lector = Lector::abrir(zip, limite)?;
    let formato = detectar(&lector, formatos)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| match e.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy) => {
                "el archivo está ocupado: inténtalo cuando acabe lo que lo usa".to_string()
            }
            _ => format!("no se pudo empezar la importación: {}", motivo(&e)),
        })?;

    let mut resumen = Resumen::default();
    let mut vistas = 0;
    let mut sin_repetir = HashSet::new();
    let mut anotar = |avisos: &mut Vec<String>, aviso: String| {
        if sin_repetir.insert(aviso.clone()) {
            avisos.push(aviso);
        }
    };
    let mut guardar = |c: ConversacionImportada| {
        vistas += 1;
        let efecto = fusionar(&tx, &c)
            .map_err(|e| format!("no se pudo guardar la n.º {vistas}: {}", motivo(&e)))?;
        match efecto {
            Efecto::Nueva => resumen.nuevas += 1,
            Efecto::Actualizada => resumen.actualizadas += 1,
            Efecto::Versionada => resumen.versionadas += 1,
            Efecto::Igual => resumen.iguales += 1,
        }
        for aviso in c.base.avisos {
            anotar(&mut resumen.avisos, aviso);
        }
        Ok(())
    };
    let mut del_formato = vec![];
    (formato.leer)(&mut lector, &mut guardar, &mut del_formato).map_err(|e| lector.situar(e))?;
    for aviso in del_formato {
        anotar(&mut resumen.avisos, aviso);
    }

    tx.commit()
        .map_err(|e| format!("no se pudo confirmar la importación: {}", motivo(&e)))?;
    Ok(resumen)
}

/// Solo el código de SQLite: el mensaje de `fusionar` puede arrastrar datos del ZIP, como el id.
fn motivo(e: &rusqlite::Error) -> String {
    match e.sqlite_error_code() {
        Some(codigo) => format!("{codigo:?}"),
        None => "datos no válidos".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::zip::{recorrer_array, recorrer_array_hasta};
    use super::*;
    use crate::archivo_web;
    use crate::boveda::LlaveMaestra;
    use crate::modelo::{ConversacionParseada, Fuente, Mensaje, Parte, Rol, TipoParte, TipoWeb};
    use ::zip::write::SimpleFileOptions;
    use ::zip::{CompressionMethod, ZipWriter};
    use serde::Deserialize;
    use serde_json::json;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Barrier;
    use std::time::Duration;
    use tempfile::{tempdir, TempDir};

    const MARCADOR: &str = "ZorroVioletaMarcador";
    const ENTRADA: &str = "conversaciones.json";

    #[derive(Deserialize)]
    struct Fila {
        id: String,
        textos: Vec<String>,
    }

    fn conversacion(f: Fila) -> ConversacionImportada {
        let mensajes = f.textos.into_iter().map(|contenido| Mensaje {
            uuid: None,
            id_padre: None,
            rol: Rol::Usuario,
            modelo: None,
            hora: None,
            oculto: false,
            partes: vec![Parte {
                tipo: TipoParte::Texto,
                contenido,
            }],
        });
        ConversacionImportada {
            base: ConversacionParseada {
                fuente: Fuente::ClaudeWeb,
                id_original: f.id,
                cwd: None,
                padre_original: None,
                descripcion: None,
                hoja: None,
                mensajes: mensajes.collect(),
                avisos: vec![],
            },
            titulo: Some("Plan de riego".to_string()),
            proyecto: None,
            inicio: None,
            fin: None,
            tipo: TipoWeb::Conversacion,
            original: None,
        }
    }

    fn tiene_entrada(nombres: &[String]) -> bool {
        nombres.iter().any(|n| n == ENTRADA)
    }

    fn leer_filas(l: &mut Lector, salida: Salida, avisos: &mut Vec<String>) -> Result<(), String> {
        recorrer_array(l.entrada(ENTRADA)?, |f: Fila| {
            if f.id == "rompe" {
                return Err("corte de prueba".to_string());
            }
            salida(conversacion(f))
        })?;
        avisos.push("aviso de prueba".to_string());
        Ok(())
    }

    const PRUEBA: Formato = Formato {
        nombre: "prueba",
        reconoce: tiene_entrada,
        leer: leer_filas,
    };

    /// Lee varias entradas seguidas: el límite es de la importación, no de cada una.
    fn leer_dos(l: &mut Lector, salida: Salida, _: &mut Vec<String>) -> Result<(), String> {
        for nombre in ["a.json", "b.json"] {
            recorrer_array(l.entrada(nombre)?, |f: Fila| salida(conversacion(f)))?;
        }
        Ok(())
    }

    const DOS_ENTRADAS: Formato = Formato {
        nombre: "dos",
        reconoce: |_| true,
        leer: leer_dos,
    };

    /// Tarda entre conversación y conversación, con la transacción ya abierta y escrita.
    fn leer_lento(l: &mut Lector, salida: Salida, _: &mut Vec<String>) -> Result<(), String> {
        recorrer_array(l.entrada(ENTRADA)?, |f: Fila| {
            salida(conversacion(f))?;
            std::thread::sleep(Duration::from_millis(80));
            Ok(())
        })
    }

    const LENTO: Formato = Formato {
        nombre: "lento",
        reconoce: tiene_entrada,
        leer: leer_lento,
    };

    fn leer_ruta_mala(l: &mut Lector, salida: Salida, _: &mut Vec<String>) -> Result<(), String> {
        recorrer_array(l.entrada("../../evil")?, |f: Fila| salida(conversacion(f)))
    }

    const RUTA_MALA: Formato = Formato {
        nombre: "ruta",
        reconoce: |_| true,
        leer: leer_ruta_mala,
    };

    /// Un tope por elemento pequeño, como el real pero alcanzable en una prueba.
    fn leer_con_tope(l: &mut Lector, salida: Salida, _: &mut Vec<String>) -> Result<(), String> {
        recorrer_array_hasta(l.entrada(ENTRADA)?, 200, |f: Fila| salida(conversacion(f)))
    }

    const CON_TOPE: Formato = Formato {
        nombre: "tope",
        reconoce: tiene_entrada,
        leer: leer_con_tope,
    };

    /// Cada conversación trae un aviso común y uno propio; el formato repite el común.
    fn leer_avisos(l: &mut Lector, salida: Salida, avisos: &mut Vec<String>) -> Result<(), String> {
        recorrer_array(l.entrada(ENTRADA)?, |f: Fila| {
            let propio = format!("aviso de {}", f.id);
            let mut c = conversacion(f);
            c.base.avisos = vec!["parte sin leer".to_string(), propio];
            salida(c)
        })?;
        avisos.push("parte sin leer".to_string());
        avisos.push("aviso del formato".to_string());
        Ok(())
    }

    const CON_AVISOS: Formato = Formato {
        nombre: "avisos",
        reconoce: tiene_entrada,
        leer: leer_avisos,
    };

    fn fila(id: &str, textos: &[&str]) -> serde_json::Value {
        json!({ "id": id, "textos": textos })
    }

    fn array(filas: &[serde_json::Value]) -> Vec<u8> {
        serde_json::to_vec(filas).unwrap()
    }

    fn crear_zip(ruta: &Path, entradas: &[(&str, &[u8])], metodo: CompressionMethod) {
        let mut z = ZipWriter::new(std::fs::File::create(ruta).unwrap());
        let op = SimpleFileOptions::default().compression_method(metodo);
        for (nombre, datos) in entradas {
            z.start_file(*nombre, op).unwrap();
            z.write_all(datos).unwrap();
        }
        z.finish().unwrap();
    }

    fn zip_de(dir: &Path, filas: &[serde_json::Value]) -> PathBuf {
        let ruta = dir.join("exportacion.zip");
        crear_zip(
            &ruta,
            &[(ENTRADA, &array(filas))],
            CompressionMethod::Deflated,
        );
        ruta
    }

    fn base() -> (TempDir, Connection) {
        let dir = tempdir().unwrap();
        let con = archivo_web::abrir(&dir.path().join("casa"), &LlaveMaestra::de_prueba(50));
        (dir, con.unwrap())
    }

    /// Filas de todas las tablas que toca una importación.
    fn estado(con: &Connection) -> [i64; 4] {
        ["conversaciones", "mensajes", "partes", "originales"].map(|t| {
            con.query_row(&format!("SELECT count(*) FROM {t}"), [], |f| f.get(0))
                .unwrap()
        })
    }

    #[test]
    fn importa_y_resume() {
        let (dir, mut con) = base();
        let zip = zip_de(
            dir.path(),
            &[fila("a", &["hola"]), fila("b", &["uno", "dos"])],
        );
        let r = importar_con(&mut con, &zip, &[PRUEBA]).unwrap();
        assert_eq!(
            r,
            Resumen {
                nuevas: 2,
                avisos: vec!["aviso de prueba".to_string()],
                ..Resumen::default()
            }
        );
        assert_eq!(estado(&con)[..3], [2, 3, 3]);
    }

    #[test]
    fn reimportar_mismo_zip_no_cambia_nada() {
        let (dir, mut con) = base();
        let zip = zip_de(dir.path(), &[fila("a", &["hola"]), fila("b", &["adios"])]);
        importar_con(&mut con, &zip, &[PRUEBA]).unwrap();
        let antes = estado(&con);
        let r = importar_con(&mut con, &zip, &[PRUEBA]).unwrap();
        assert_eq!((r.nuevas, r.actualizadas, r.versionadas), (0, 0, 0));
        assert_eq!(r.iguales, 2);
        assert_eq!(estado(&con), antes);
    }

    #[test]
    fn distingue_actualizada_y_versionada() {
        let (dir, mut con) = base();
        let primero = zip_de(dir.path(), &[fila("a", &["uno"]), fila("b", &["dos"])]);
        importar_con(&mut con, &primero, &[PRUEBA]).unwrap();
        let segundo = zip_de(
            dir.path(),
            &[fila("a", &["uno", "más"]), fila("b", &["otro"])],
        );
        let r = importar_con(&mut con, &segundo, &[PRUEBA]).unwrap();
        assert_eq!((r.actualizadas, r.versionadas), (1, 1));
    }

    #[test]
    fn zip_truncado_no_cambia_nada() {
        let (dir, mut con) = base();
        let zip = zip_de(dir.path(), &[fila("a", &["hola"])]);
        importar_con(&mut con, &zip, &[PRUEBA]).unwrap();
        let antes = estado(&con);

        let zip = zip_de(dir.path(), &[fila("b", &["nueva"]), fila("c", &["otra"])]);
        let bytes = std::fs::read(&zip).unwrap();
        std::fs::write(&zip, &bytes[..bytes.len() / 2]).unwrap();
        assert!(importar_con(&mut con, &zip, &[PRUEBA]).is_err());
        assert_eq!(estado(&con), antes);
    }

    /// El directorio del ZIP está bien y el daño queda al final del dato: solo lo delata la
    /// suma de comprobación, cuando las conversaciones ya se han escrito.
    #[test]
    fn dato_danado_al_final_revierte_todo() {
        let (dir, mut con) = base();
        let ruta = dir.path().join("danado.zip");
        let json = array(&[fila("a", &["hola"]), fila("b", &["adios"])]);
        crear_zip(&ruta, &[(ENTRADA, &json)], CompressionMethod::Stored);
        let mut bytes = std::fs::read(&ruta).unwrap();
        // Dentro del último texto: sigue siendo JSON válido, pero la suma ya no cuadra.
        let texto = bytes.windows(5).rposition(|v| v == b"adios").unwrap();
        bytes[texto] = b'o';
        std::fs::write(&ruta, bytes).unwrap();

        assert!(importar_con(&mut con, &ruta, &[PRUEBA]).is_err());
        assert_eq!(estado(&con), [0; 4]);
    }

    #[test]
    fn formato_desconocido_lo_dice_sin_contenido() {
        let (dir, mut con) = base();
        let ruta = dir.path().join("otro.zip");
        let nombre = format!("{MARCADOR}.json");
        crear_zip(
            &ruta,
            &[(nombre.as_str(), MARCADOR.as_bytes())],
            CompressionMethod::Deflated,
        );
        for e in [
            importar_con(&mut con, &ruta, &[PRUEBA]).unwrap_err(),
            reconocer_con(&ruta, &[PRUEBA]).unwrap_err(),
            reconocer(&ruta).unwrap_err(),
        ] {
            assert!(e.contains("desconocido"), "{e}");
            assert!(e.contains("Claude.ai"), "{e}");
            assert!(e.contains("ChatGPT"), "{e}");
            assert!(!e.contains(MARCADOR), "{e}");
        }
        assert_eq!(estado(&con), [0; 4]);
    }

    #[test]
    fn reconocer_da_el_nombre_y_no_escribe_nada() {
        let dir = tempdir().unwrap();
        let zip = zip_de(dir.path(), &[fila("a", &["hola"])]);
        assert_eq!(reconocer_con(&zip, &[PRUEBA]).unwrap(), "prueba");
        let archivos = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(archivos, 1);
    }

    #[test]
    fn un_archivo_que_no_es_zip_se_rechaza() {
        let (dir, mut con) = base();
        let ruta = dir.path().join("falso.zip");
        std::fs::write(&ruta, MARCADOR).unwrap();
        let e = importar_con(&mut con, &ruta, &[PRUEBA]).unwrap_err();
        assert!(!e.contains(MARCADOR), "{e}");
        assert!(reconocer_con(&ruta, &[PRUEBA]).is_err());
    }

    #[test]
    fn fallo_a_mitad_revierte_todo() {
        let (dir, mut con) = base();
        let zip = zip_de(
            dir.path(),
            &[
                fila("a", &["hola"]),
                fila("b", &["adios"]),
                fila("rompe", &[]),
            ],
        );
        let e = importar_con(&mut con, &zip, &[PRUEBA]).unwrap_err();
        assert!(e.contains("corte de prueba"), "{e}");
        assert_eq!(estado(&con), [0; 4]);
    }

    /// Las firmas son las de la especificación de ZIP; cambia los dos sitios donde se declara el
    /// tamaño, porque la biblioteca rechaza un ZIP en el que no coincidan.
    fn declarar_tamano(bytes: &mut [u8], tamano: u32) {
        for i in 0..bytes.len() - 4 {
            let campo = match &bytes[i..i + 4] {
                b"PK\x03\x04" => i + 22,
                b"PK\x01\x02" => i + 24,
                _ => continue,
            };
            bytes[campo..campo + 4].copy_from_slice(&tamano.to_le_bytes());
        }
    }

    #[test]
    fn tamano_real_supera_limite() {
        let (dir, mut con) = base();
        let relleno = "a".repeat(100_000);
        let zip = zip_de(
            dir.path(),
            &[fila("a", &["hola"]), fila("b", &[relleno.as_str()])],
        );
        let mut bytes = std::fs::read(&zip).unwrap();
        declarar_tamano(&mut bytes, 40);
        std::fs::write(&zip, bytes).unwrap();

        // Solo prueba que falla sin escribir: la biblioteca corta al pasar del tamaño declarado,
        // antes de llegar a `limite`. El contador lo prueba `el_limite_suma_todas_las_entradas`.
        assert!(importar_hasta(&mut con, &zip, &[PRUEBA], 1_000).is_err());
        assert_eq!(estado(&con), [0; 4]);
    }

    #[test]
    fn el_limite_suma_todas_las_entradas() {
        let (dir, mut con) = base();
        let ruta = dir.path().join("dos.zip");
        let a = array(&[fila("a", &["uno"])]);
        let b = array(&[fila("b", &["dos"])]);
        crear_zip(
            &ruta,
            &[("a.json", &a), ("b.json", &b)],
            CompressionMethod::Deflated,
        );
        let total = (a.len() + b.len()) as u64;

        let e = importar_hasta(&mut con, &ruta, &[DOS_ENTRADAS], total - 1).unwrap_err();
        assert!(e.contains("límite"), "{e}");
        assert!(e.contains("b.json"), "{e}");
        assert_eq!(estado(&con), [0; 4]);

        let r = importar_hasta(&mut con, &ruta, &[DOS_ENTRADAS], total).unwrap();
        assert_eq!(r.nuevas, 2);
    }

    #[test]
    fn conversacion_repetida_en_un_zip() {
        let (dir, mut con) = base();
        let zip = zip_de(dir.path(), &[fila("a", &["hola"]), fila("a", &["hola"])]);
        let r = importar_con(&mut con, &zip, &[PRUEBA]).unwrap();
        assert_eq!((r.nuevas, r.iguales), (1, 1));
        assert_eq!(estado(&con)[0], 1);

        let (dir, mut con) = base();
        let zip = zip_de(
            dir.path(),
            &[fila("a", &["hola"]), fila("a", &["hola", "sigue"])],
        );
        let r = importar_con(&mut con, &zip, &[PRUEBA]).unwrap();
        assert_eq!((r.nuevas, r.actualizadas), (1, 1));
        assert_eq!(estado(&con)[..2], [1, 2]);
    }

    #[test]
    fn ruta_maliciosa_no_escribe_fuera() {
        let (dir, mut con) = base();
        let hondo = dir.path().join("x/y");
        std::fs::create_dir_all(&hondo).unwrap();
        let ruta = hondo.join("malo.zip");
        crear_zip(
            &ruta,
            &[("../../evil", &array(&[fila("a", &["hola"])]))],
            CompressionMethod::Deflated,
        );
        let r = importar_con(&mut con, &ruta, &[RUTA_MALA]).unwrap();
        assert_eq!(r.nuevas, 1);
        for sitio in [dir.path(), &dir.path().join("x"), &hondo] {
            assert!(!sitio.join("evil").exists());
        }
        assert!(!Path::new("../../evil").exists());
    }

    /// SQLite borra sus temporales nada más crearlos, así que una carpeta vacía no basta: el
    /// hijo también comprueba que la conexión los guarda en memoria.
    #[test]
    #[ignore = "lo lanza no_escribe_en_la_carpeta_temporal en un proceso aparte"]
    fn hijo_importa_con_tmpdir_propio() {
        let casa = std::env::var_os("NISABA_PRUEBA_CASA").unwrap();
        let zip = std::env::var_os("NISABA_PRUEBA_ZIP").unwrap();
        let mut con = archivo_web::abrir(Path::new(&casa), &LlaveMaestra::de_prueba(51)).unwrap();
        let en_memoria: i64 = con
            .query_row("PRAGMA temp_store", [], |f| f.get(0))
            .unwrap();
        assert_eq!(en_memoria, 2);
        let r = importar_con(&mut con, Path::new(&zip), &[PRUEBA]).unwrap();
        assert_eq!(r.nuevas, 2000);
    }

    #[test]
    fn elemento_gigante_revierte_lo_ya_importado() {
        let (dir, mut con) = base();
        let relleno = "a".repeat(1_000);
        let zip = zip_de(
            dir.path(),
            &[fila("a", &["hola"]), fila("b", &[relleno.as_str()])],
        );
        let e = importar_con(&mut con, &zip, &[CON_TOPE]).unwrap_err();
        assert!(e.contains("límite"), "{e}");
        assert!(e.contains(ENTRADA), "{e}");
        assert_eq!(estado(&con), [0; 4]);
    }

    #[test]
    fn avisos_de_conversacion_llegan_al_resumen() {
        let (dir, mut con) = base();
        let zip = zip_de(dir.path(), &[fila("a", &["hola"]), fila("b", &["adios"])]);
        let r = importar_con(&mut con, &zip, &[CON_AVISOS]).unwrap();
        assert_eq!(
            r.avisos,
            [
                "parte sin leer",
                "aviso de a",
                "aviso de b",
                "aviso del formato"
            ]
        );
    }

    #[test]
    fn archivo_ocupado_lo_dice_y_no_escribe() {
        let (dir, mut con) = base();
        let llave = LlaveMaestra::de_prueba(50);
        let otra = archivo_web::abrir(&dir.path().join("casa"), &llave).unwrap();
        otra.execute_batch("BEGIN IMMEDIATE").unwrap();
        con.busy_timeout(Duration::from_millis(20)).unwrap();

        let zip = zip_de(dir.path(), &[fila("a", &["hola"])]);
        let e = importar_con(&mut con, &zip, &[PRUEBA]).unwrap_err();
        assert!(e.contains("ocupado"), "{e}");
        otra.execute_batch("ROLLBACK").unwrap();
        assert_eq!(estado(&con), [0; 4]);
    }

    /// Una conexión sin `temp_store = MEMORY` podría dejar texto claro en la carpeta temporal.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "temp_store")]
    fn una_conexion_con_temporales_en_disco_se_rechaza_al_empezar() {
        let dir = tempdir().unwrap();
        let zip = zip_de(dir.path(), &[fila("a", &["hola"])]);
        let mut con = Connection::open_in_memory().unwrap();
        let _ = importar_con(&mut con, &zip, &[PRUEBA]);
    }

    /// `TMPDIR` es de todo el proceso: cambiarlo aquí afectaría a las pruebas paralelas. Por eso
    /// la importación corre en un hijo, el mismo binario de pruebas con una sola prueba.
    #[test]
    fn no_escribe_en_la_carpeta_temporal() {
        let dir = tempdir().unwrap();
        let temporal = tempdir().unwrap();
        let textos = format!("{MARCADOR} {}", "relleno ".repeat(50));
        let filas: Vec<_> = (0..2000)
            .map(|i| fila(&format!("c{i}"), &[textos.as_str()]))
            .collect();
        let zip = zip_de(dir.path(), &filas);

        let salida = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "importar::tests::hijo_importa_con_tmpdir_propio",
                "--ignored",
            ])
            .env("TMPDIR", temporal.path())
            .env("SQLITE_TMPDIR", temporal.path())
            .env("NISABA_PRUEBA_CASA", dir.path().join("casa"))
            .env("NISABA_PRUEBA_ZIP", &zip)
            .output()
            .unwrap();
        let texto = String::from_utf8_lossy(&salida.stdout);
        assert!(salida.status.success(), "{texto}");
        assert!(texto.contains("1 passed"), "{texto}");

        let restos: Vec<_> = std::fs::read_dir(temporal.path()).unwrap().collect();
        assert!(restos.is_empty(), "{restos:?}");
    }

    /// Importaciones breves: la espera dura lo que `busy_timeout`, y si no alcanza lo dice
    /// `archivo_ocupado_lo_dice_y_no_escribe`.
    #[test]
    fn dos_importaciones_breves_se_turnan() {
        let dir = tempdir().unwrap();
        let casa = dir.path().join("casa");
        let filas: Vec<_> = (0..4).map(|i| fila(&format!("c{i}"), &["hola"])).collect();
        let zip = zip_de(dir.path(), &filas);
        let llave = LlaveMaestra::de_prueba(52);
        drop(archivo_web::abrir(&casa, &llave).unwrap());

        let salida = Barrier::new(2);
        let resultados: Vec<_> = std::thread::scope(|s| {
            let hilos: Vec<_> = (0..2)
                .map(|_| {
                    s.spawn(|| {
                        let mut con = archivo_web::abrir(&casa, &llave).unwrap();
                        salida.wait();
                        importar_con(&mut con, &zip, &[LENTO])
                    })
                })
                .collect();
            hilos.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let resumenes: Vec<_> = resultados.into_iter().map(Result::unwrap).collect();
        // Una entra entera y la otra, al acabar la primera, ya lo encuentra todo.
        assert_eq!(resumenes.iter().map(|r| r.nuevas).sum::<usize>(), 4);
        assert_eq!(resumenes.iter().map(|r| r.iguales).sum::<usize>(), 4);
    }

    #[test]
    fn un_error_de_json_nombra_la_entrada_sin_citar_contenido() {
        let (dir, mut con) = base();
        let ruta = dir.path().join("malo.zip");
        let json = format!(r#"[{{"id":"a","textos":"{MARCADOR}"}}]"#);
        crear_zip(
            &ruta,
            &[(ENTRADA, json.as_bytes())],
            CompressionMethod::Deflated,
        );
        let e = importar_con(&mut con, &ruta, &[PRUEBA]).unwrap_err();
        assert!(e.contains(ENTRADA), "{e}");
        assert!(e.contains("columna"), "{e}");
        assert!(!e.contains(MARCADOR), "{e}");
    }

    #[test]
    fn un_id_con_arroba_revierte_sin_salir_en_el_error() {
        let (dir, mut con) = base();
        let id = format!("{MARCADOR}@1");
        let zip = zip_de(dir.path(), &[fila("a", &["hola"]), fila(&id, &["adios"])]);
        let e = importar_con(&mut con, &zip, &[PRUEBA]).unwrap_err();
        assert!(!e.contains(MARCADOR), "{e}");
        assert!(e.contains('2'), "dice cuál de ellas: {e}");
        assert_eq!(estado(&con), [0; 4]);
    }

    #[test]
    fn falta_la_entrada_que_pide_el_formato() {
        let (dir, mut con) = base();
        let zip = zip_de(dir.path(), &[fila("a", &["hola"])]);
        let e = importar_con(&mut con, &zip, &[RUTA_MALA]).unwrap_err();
        assert!(e.contains("../../evil"), "{e}");
        assert!(e.contains("no está"), "{e}");
    }

    #[test]
    fn el_texto_no_queda_en_claro_en_disco() {
        let (dir, mut con) = base();
        let zip = zip_de(dir.path(), &[fila("a", &[MARCADOR])]);
        importar_con(&mut con, &zip, &[PRUEBA]).unwrap();
        std::fs::remove_file(zip).unwrap();
        for e in std::fs::read_dir(dir.path().join("casa")).unwrap() {
            let bytes = std::fs::read(e.unwrap().path()).unwrap();
            assert!(!bytes
                .windows(MARCADOR.len())
                .any(|v| v == MARCADOR.as_bytes()));
        }
        assert_eq!(estado(&con)[0], 1);
    }

    #[test]
    fn sin_formatos_todavia_todo_es_desconocido() {
        let dir = tempdir().unwrap();
        let zip = zip_de(dir.path(), &[fila("a", &["hola"])]);
        assert!(reconocer(&zip).unwrap_err().contains("desconocido"));
    }
}
