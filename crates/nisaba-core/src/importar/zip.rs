//! Lectura de un ZIP que no es de fiar: nada se extrae a disco y todo lo leído cuenta contra dos
//! límites. Lo que el ZIP declare de sí mismo no se usa para decidir nada.

use std::cell::Cell;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::marker::PhantomData;
use std::path::Path;

use ::zip::result::ZipError;
use ::zip::ZipArchive;
use serde::de::{DeserializeOwned, Deserializer, Error as _, SeqAccess, Visitor};
use serde_json::error::Category;

/// Tope de bytes descomprimidos por importación. Frena una bomba ZIP en tiempo y en disco (lo
/// leído acaba en la tablilla), pero no protege la memoria: para eso está `LIMITE_ELEMENTO`.
pub(crate) const LIMITE_TOTAL: u64 = 20 * 1024 * 1024 * 1024;

/// Tope de bytes de un solo elemento de `recorrer_array`. Cada elemento se carga entero en
/// memoria, así que este límite, y no el total, es el que impide que una cadena de 20 GiB tumbe
/// el proceso. Sobra para cualquier conversación real.
pub(crate) const LIMITE_ELEMENTO: u64 = 1024 * 1024 * 1024;

/// El ZIP abierto más lo leído hasta ahora. El contador es de toda la importación: repartir una
/// bomba entre muchas entradas, o volver a abrir la misma, no lo reinicia.
pub(crate) struct Lector {
    archivo: ZipArchive<File>,
    leidos: u64,
    limite: u64,
    /// La última entrada pedida, para situar los errores.
    actual: Option<String>,
}

impl Lector {
    /// Los errores de la biblioteca no se reenvían: no hacen falta y describen un archivo ajeno.
    pub fn abrir(zip: &Path, limite: u64) -> Result<Self, String> {
        Self::abrir_con_leidos(zip, 0, limite)
    }

    pub fn abrir_con_leidos(zip: &Path, leidos: u64, limite: u64) -> Result<Self, String> {
        let archivo = File::open(zip).map_err(|e| format!("no se pudo abrir el ZIP: {e}"))?;
        let archivo = ZipArchive::new(archivo)
            .map_err(|_| "el archivo está dañado o no es un ZIP".to_string())?;
        Ok(Lector {
            archivo,
            leidos,
            limite,
            actual: None,
        })
    }

    pub fn leidos(&self) -> u64 {
        self.leidos
    }

    /// Un nombre que no se puede decodificar no se ofrece: ningún formato lo busca.
    pub fn nombres(&self) -> Vec<String> {
        self.archivo
            .file_names()
            .filter_map(|n| n.ok().map(|n| n.into_owned()))
            .collect()
    }

    /// Lee los primeros bytes de una entrada para distinguir formatos que comparten nombre
    /// (`conversations.json`), sin alterar el contador ni `actual`.
    pub fn muestra(&mut self, nombre: &str, tope: usize) -> Option<Vec<u8>> {
        let entrada = self.archivo.by_name(nombre).ok()?;
        let mut buf = Vec::with_capacity(tope.min(4096));
        entrada.take(tope as u64).read_to_end(&mut buf).ok()?;
        Some(buf)
    }

    /// Abre por nombre exacto. Es seguro con cualquier nombre (`../../evil` incluido): solo se
    /// usa para encontrar la entrada, nunca para decidir una ruta, porque nada va a disco.
    pub fn entrada(&mut self, nombre: &str) -> Result<impl Read + '_, String> {
        self.actual = Some(nombre.to_string());
        let Lector {
            archivo,
            leidos,
            limite,
            ..
        } = self;
        let interior = archivo.by_name(nombre).map_err(|e| match e {
            ZipError::FileNotFound => "no está en el ZIP".to_string(),
            _ => "entrada dañada o no admitida".to_string(),
        })?;
        Ok(Contador {
            interior,
            leidos,
            limite: *limite,
        })
    }

    /// Lee una entrada entera como texto UTF-8 aplicando tanto el tope de elemento (`1 GiB`) como
    /// el tope total de la importación (`20 GiB`).
    pub fn leer_cadena(&mut self, nombre: &str) -> Result<String, String> {
        let r = self.entrada(nombre)?;
        let usados = Cell::new(0);
        let mut lector = PorElemento {
            interior: r,
            usados: &usados,
            tope: LIMITE_ELEMENTO,
        };
        let mut texto = String::new();
        lector
            .read_to_string(&mut texto)
            .map_err(|e| format!("no se pudo leer: {e}"))?;
        Ok(texto)
    }

    /// Antepone la entrada que se estaba leyendo.
    pub fn situar(&self, error: String) -> String {
        match &self.actual {
            Some(entrada) => format!("{entrada}: {error}"),
            None => error,
        }
    }
}

/// Cuenta lo que realmente sale del ZIP, sin fiarse de lo que declaran sus cabeceras.
struct Contador<'a, R> {
    interior: R,
    leidos: &'a mut u64,
    limite: u64,
}

impl<R: Read> Read for Contador<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        const AVISO: &str = "el contenido supera el límite de tamaño";
        leer_con_tope(&mut self.interior, buf, self.leidos, self.limite, AVISO)
    }
}

/// Como `Contador`, pero para un solo elemento: `recorrer_array` lo pone a cero entre uno y otro.
/// `Cell` porque lo tocan a la vez este lector, dentro de serde, y el visitante.
struct PorElemento<'a, R> {
    interior: R,
    usados: &'a Cell<u64>,
    tope: u64,
}

impl<R: Read> Read for PorElemento<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        const AVISO: &str = "un elemento supera el límite de tamaño";
        let mut usados = self.usados.get();
        let leido = leer_con_tope(&mut self.interior, buf, &mut usados, self.tope, AVISO);
        self.usados.set(usados);
        leido
    }
}

fn leer_con_tope(
    r: &mut impl Read,
    buf: &mut [u8],
    usados: &mut u64,
    tope: u64,
    aviso: &'static str,
) -> io::Result<usize> {
    // Se pide un byte más de lo que queda: basta para saber que se pasa sin leer de más.
    let quedan = tope.saturating_sub(*usados).saturating_add(1);
    let pedir = usize::try_from(quedan).map_or(buf.len(), |q| q.min(buf.len()));
    let n = r.read(&mut buf[..pedir])?;
    *usados += n as u64;
    if *usados > tope {
        return Err(io::Error::new(io::ErrorKind::InvalidData, aviso));
    }
    Ok(n)
}

/// Recorre un array JSON elemento a elemento, sin cargarlo entero: puede ocupar cientos de MB.
/// Cada elemento sí pasa por memoria, y por eso lleva su propio tope.
/// Comprueba además que no haya nada detrás, y esa última lectura es la que llega al final de
/// la entrada y hace saltar la suma de comprobación del ZIP.
pub(crate) fn recorrer_array<T: DeserializeOwned>(
    r: impl Read,
    f: impl FnMut(T) -> Result<(), String>,
) -> Result<(), String> {
    recorrer_array_hasta(r, LIMITE_ELEMENTO, f)
}

pub(crate) fn recorrer_array_hasta<T: DeserializeOwned>(
    r: impl Read,
    tope: u64,
    mut f: impl FnMut(T) -> Result<(), String>,
) -> Result<(), String> {
    // Va por encima del búfer: así cuenta lo que entrega a serde, no lo que el búfer adelanta y
    // aún pertenece al elemento siguiente. Cuenta desde el primer byte: si lo que hay no es un
    // array, serde también lo carga para decir qué encontró.
    let usados = Cell::new(0);
    let lector = PorElemento {
        interior: BufReader::new(r),
        usados: &usados,
        tope,
    };
    let mut json = serde_json::Deserializer::from_reader(lector);
    let mut fallo = None;
    let visita = Visita {
        f: &mut f,
        usados: &usados,
        fallo: &mut fallo,
        tipo: PhantomData,
    };
    let leido = (&mut json)
        .deserialize_seq(visita)
        .and_then(|()| json.end());
    // Lo de `f` va aparte: serde no puede devolverlo sin convertirlo en uno de sus errores.
    match fallo {
        Some(error) => Err(error),
        None => leido.map_err(describir),
    }
}

struct Visita<'a, T, F> {
    f: &'a mut F,
    usados: &'a Cell<u64>,
    fallo: &'a mut Option<String>,
    tipo: PhantomData<fn() -> T>,
}

impl<'de, T, F> Visitor<'de> for Visita<'_, T, F>
where
    T: DeserializeOwned,
    F: FnMut(T) -> Result<(), String>,
{
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("un array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        loop {
            self.usados.set(0);
            let Some(elemento) = seq.next_element::<T>()? else {
                return Ok(());
            };
            if let Err(error) = (self.f)(elemento) {
                *self.fallo = Some(error);
                return Err(A::Error::custom("corte"));
            }
        }
    }
}

/// El mensaje de serde puede citar el dato (`invalid type: string "..."`): solo se da la posición.
fn describir(e: serde_json::Error) -> String {
    let (linea, columna) = (e.line(), e.column());
    match e.classify() {
        Category::Io => format!("no se pudo leer: {}", io::Error::from(e)),
        Category::Eof => format!("JSON incompleto (línea {linea}, columna {columna})"),
        Category::Syntax | Category::Data => {
            format!("JSON no válido (línea {linea}, columna {columna})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::io::Cursor;

    const MARCADOR: &str = "ZorroVioletaMarcador";

    #[derive(Deserialize)]
    struct Fila {
        n: u32,
    }

    fn numeros(json: &str) -> Result<Vec<u32>, String> {
        let mut vistos = vec![];
        recorrer_array(json.as_bytes(), |f: Fila| {
            vistos.push(f.n);
            Ok(())
        })?;
        Ok(vistos)
    }

    #[test]
    fn recorre_en_orden_uno_a_uno() {
        assert_eq!(numeros(r#"[{"n":1}, {"n":2}]"#).unwrap(), vec![1, 2]);
        assert_eq!(numeros("[]").unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn el_error_de_quien_recibe_llega_intacto_y_corta() {
        let mut vistos = 0;
        let e = recorrer_array(r#"[{"n":1},{"n":2}]"#.as_bytes(), |_: Fila| {
            vistos += 1;
            Err("parada".to_string())
        })
        .unwrap_err();
        assert_eq!(e, "parada");
        assert_eq!(vistos, 1);
    }

    #[test]
    fn un_error_de_json_da_la_posicion_y_no_cita_el_contenido() {
        for json in [
            format!(r#"[{{"n":"{MARCADOR}"}}]"#),
            format!(r#"{{"{MARCADOR}":1}}"#),
            format!(r#"[{{"n":1,"{MARCADOR}"#),
            format!(r#"[{{"n":1}}] {MARCADOR}"#),
            format!(r#""{MARCADOR}""#),
        ] {
            let e = numeros(&json).unwrap_err();
            assert!(!e.contains(MARCADOR), "{e}");
            assert!(e.contains("línea 1"), "{e}");
        }
    }

    #[test]
    fn basura_tras_el_array_es_un_error() {
        assert!(numeros(r#"[{"n":1}] x"#).is_err());
        assert!(numeros(r#"[{"n":1}]  "#).is_ok());
    }

    #[test]
    fn un_fallo_de_lectura_se_nota_aunque_el_array_ya_haya_cerrado() {
        struct FallaAlFinal(Cursor<&'static [u8]>);
        impl Read for FallaAlFinal {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                match self.0.read(buf)? {
                    0 => Err(io::Error::other("suma incorrecta")),
                    n => Ok(n),
                }
            }
        }
        let r = FallaAlFinal(Cursor::new(br#"[{"n":1}]"#));
        let e = recorrer_array(r, |_: Fila| Ok(())).unwrap_err();
        assert!(e.contains("suma incorrecta"), "{e}");
    }

    /// Un JSON enorme que se genera según se pide, sin ocupar memoria en la prueba, y anota
    /// cuánto se le ha sacado.
    struct Generador<'a> {
        inicio: &'static [u8],
        total: u64,
        sacados: &'a Cell<u64>,
    }

    impl Read for Generador<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let quedan = self.total - self.sacados.get();
            let n = buf.len().min(usize::try_from(quedan).unwrap_or(usize::MAX));
            buf[..n].fill(b'a');
            let del_inicio = self.inicio.len().min(n);
            buf[..del_inicio].copy_from_slice(&self.inicio[..del_inicio]);
            self.inicio = &self.inicio[del_inicio..];
            self.sacados.set(self.sacados.get() + n as u64);
            Ok(n)
        }
    }

    const TOPE: u64 = 1024 * 1024;

    /// Lo que importa es cuánto se sacó del lector, no solo que falle: sin tope, serde se
    /// tragaría los 64 MiB antes de notar que la cadena no acaba nunca.
    fn sacados_hasta_cortar(inicio: &'static [u8]) -> (String, u64) {
        let sacados = Cell::new(0);
        let r = Generador {
            inicio,
            total: 64 * TOPE,
            sacados: &sacados,
        };
        let e = recorrer_array_hasta(r, TOPE, |_: Fila| Ok(())).unwrap_err();
        (e, sacados.get())
    }

    #[test]
    fn elemento_gigante_se_corta_sin_agotar_memoria() {
        let (e, sacados) = sacados_hasta_cortar(br#"[{"n":1},{"n":""#);
        assert!(e.contains("límite"), "{e}");
        assert!(sacados <= TOPE + 16 * 1024, "sacó {sacados} bytes");
    }

    #[test]
    fn una_cadena_gigante_en_lugar_del_array_tambien_se_corta() {
        let (e, sacados) = sacados_hasta_cortar(br#"""#);
        assert!(e.contains("límite"), "{e}");
        assert!(sacados <= TOPE + 16 * 1024, "sacó {sacados} bytes");
    }

    #[test]
    fn cada_elemento_estrena_su_tope() {
        let json = br#"[{"n":1},{"n":2},{"n":3},{"n":4}]"#;
        let mut vistos = 0;
        recorrer_array_hasta(&json[..], 10, |_: Fila| {
            vistos += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(vistos, 4);
        let e = recorrer_array_hasta(&json[..], 4, |_: Fila| Ok(())).unwrap_err();
        assert!(e.contains("límite"), "{e}");
    }

    #[test]
    fn el_tope_real_es_un_gibibyte() {
        assert_eq!(LIMITE_ELEMENTO, 1 << 30);
    }

    #[test]
    fn el_contador_corta_al_pasar_del_limite_y_no_antes() {
        let leer = |tam: usize, limite: u64, leidos: &mut u64| {
            let mut c = Contador {
                interior: Cursor::new(vec![0u8; tam]),
                leidos,
                limite,
            };
            let mut todo = vec![];
            c.read_to_end(&mut todo).map(|_| todo.len())
        };
        let mut leidos = 0;
        assert_eq!(leer(10, 10, &mut leidos).unwrap(), 10);
        assert_eq!(leidos, 10);
        let e = leer(1, 10, &mut leidos).unwrap_err().to_string();
        assert!(e.contains("límite"), "{e}");

        let mut leidos = 0;
        assert!(leer(1_000_000, 10, &mut leidos).is_err());
        assert!(leidos <= 11, "leyó {leidos} bytes de más");
    }
}
