//! Bóveda de llaves del archivo web.
//!
//! SQLCipher necesita una llave fija, pero una contraseña debe poder cambiar y perderse. Por eso
//! la llave maestra es aleatoria y nunca cambia, y `boveda.json` guarda dos copias cifradas de
//! ella (envolturas): una que abre la contraseña y otra que abre el código de recuperación.
//! Cambiar la contraseña reescribe su envoltura sin tocar ni un byte de `tablilla-web.db`.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit};
use argon2::{Algorithm, Argon2, Params, Version};
use data_encoding::{BASE32, BASE64, HEXLOWER};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const BOVEDA: &str = "boveda.json";
pub const TABLILLA_WEB: &str = "tablilla-web.db";

const VERSION: u32 = 1;
const CONTRASENA_MINIMA: usize = 12;
const BYTES_LLAVE: usize = 32;
const BYTES_CODIGO: usize = 20;
const BYTES_SAL: usize = 16;
const BYTES_NONCE: usize = 12;
/// La llave más los 16 bytes de la etiqueta de GCM.
const BYTES_CIFRADO: usize = BYTES_LLAVE + 16;
const CARACTERES_POR_GRUPO: usize = 4;

// El diseño parte de m=128 MiB, t=3, que aquí abría en 0,26 s (Ryzen 5 5600G, release). Se sube
// `t` hasta ~1 s y no `m`: mantiene la memoria prevista también en portátiles modestos.
const PARAMETROS: Parametros = Parametros {
    m_kib: 128 * 1024,
    t: 14,
    p: 1,
};

// Los parámetros viven en un archivo que se puede corromper: sin tope, un valor absurdo dejaría
// la apertura colgada o sin memoria en vez de avisar. Sobra margen para subirlos.
const M_MAXIMA_KIB: u32 = 4 * 1024 * 1024;
const T_MAXIMO: u32 = 64;

/// Los 32 bytes que abren `tablilla-web.db`. Se borran de memoria al soltarla.
pub struct LlaveMaestra(Zeroizing<[u8; BYTES_LLAVE]>);

impl LlaveMaestra {
    /// En hexadecimal, para el `PRAGMA key = "x'…'"` de SQLCipher.
    pub fn hex(&self) -> Zeroizing<String> {
        Zeroizing::new(HEXLOWER.encode(self.0.as_slice()))
    }
}

// A mano: un `derive` imprimiría la llave en cualquier `{:?}` o `unwrap` que falle.
impl fmt::Debug for LlaveMaestra {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LlaveMaestra(oculta)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorBoveda {
    NoExiste,
    YaExiste,
    Incorrecta,
    ContrasenaCorta,
    Danada(String),
    Io(String),
}

impl fmt::Display for ErrorBoveda {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoExiste => write!(f, "Todavía no hay archivo web en esta carpeta."),
            Self::YaExiste => write!(
                f,
                "Ya hay un archivo web en esta carpeta; no se puede crear otro encima."
            ),
            Self::Incorrecta => write!(f, "Contraseña o código de recuperación incorrectos."),
            Self::ContrasenaCorta => write!(
                f,
                "La contraseña debe tener al menos {CONTRASENA_MINIMA} caracteres."
            ),
            Self::Danada(detalle) => write!(f, "El archivo {BOVEDA} está dañado: {detalle}."),
            Self::Io(detalle) => write!(f, "No puedo usar {BOVEDA}: {detalle}."),
        }
    }
}

impl std::error::Error for ErrorBoveda {}

fn danada(detalle: &str) -> ErrorBoveda {
    ErrorBoveda::Danada(detalle.to_string())
}

fn io(e: io::Error) -> ErrorBoveda {
    ErrorBoveda::Io(e.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Parametros {
    m_kib: u32,
    t: u32,
    p: u32,
}

impl Parametros {
    fn argon2(self) -> Result<Argon2<'static>, ErrorBoveda> {
        if self.m_kib > M_MAXIMA_KIB || self.t > T_MAXIMO {
            return Err(danada("parámetros de Argon2 fuera de rango"));
        }
        let params = Params::new(self.m_kib, self.t, self.p, Some(BYTES_LLAVE))
            .map_err(|_| danada("parámetros de Argon2 inválidos"))?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }
}

/// La llave maestra cifrada con una llave derivada de un secreto. Todo en base64.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Envoltura {
    sal: String,
    nonce: String,
    cifrado: String,
}

type Partes = ([u8; BYTES_SAL], [u8; BYTES_NONCE], [u8; BYTES_CIFRADO]);

impl Envoltura {
    fn partes(&self) -> Result<Partes, ErrorBoveda> {
        Ok((
            decodificar(&self.sal, "sal")?,
            decodificar(&self.nonce, "nonce")?,
            decodificar(&self.cifrado, "cifrado")?,
        ))
    }
}

fn decodificar<const N: usize>(base64: &str, campo: &str) -> Result<[u8; N], ErrorBoveda> {
    BASE64
        .decode(base64.as_bytes())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| ErrorBoveda::Danada(format!("campo «{campo}» inválido")))
}

/// El contenido de `boveda.json`. No lleva ningún secreto en claro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Boveda {
    version: u32,
    argon2: Parametros,
    contrasena: Envoltura,
    codigo: Envoltura,
}

/// Creación en dos pasos: nada se escribe hasta confirmar el código.
pub struct Nueva {
    llave: LlaveMaestra,
    codigo: Zeroizing<String>,
    boveda: Boveda,
}

impl fmt::Debug for Nueva {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Nueva(oculta)")
    }
}

impl Nueva {
    /// El código de recuperación, por grupos. Esta es la única vez que existe en claro.
    pub fn codigo(&self) -> &str {
        &self.codigo
    }

    /// Escribe `boveda.json`. Se niega si ya hay bóveda o tablilla: pisarlas con otra llave
    /// dejaría el archivo existente ilegible para siempre.
    pub fn guardar(self, home: &Path) -> Result<LlaveMaestra, ErrorBoveda> {
        for resto in [
            BOVEDA.to_string(),
            TABLILLA_WEB.to_string(),
            format!("{TABLILLA_WEB}-wal"),
            format!("{TABLILLA_WEB}-shm"),
        ] {
            if existe(&home.join(resto))? {
                return Err(ErrorBoveda::YaExiste);
            }
        }
        escribir(home, &self.boveda, colocar_sin_pisar)?;
        Ok(self.llave)
    }
}

/// `symlink_metadata` y no `Path::exists`, que responde `false` también ante un error de permisos
/// o un enlace roto, justo los casos en que no hay que dar por libre el sitio.
fn existe(ruta: &Path) -> Result<bool, ErrorBoveda> {
    match fs::symlink_metadata(ruta) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io(e)),
    }
}

/// Genera llave y código y los envuelve. Tarda dos derivaciones de Argon2 y no toca el disco.
pub fn preparar(contrasena: &str) -> Result<Nueva, ErrorBoveda> {
    preparar_con(contrasena, PARAMETROS)
}

pub(crate) fn preparar_con(contrasena: &str, argon2: Parametros) -> Result<Nueva, ErrorBoveda> {
    comprobar_longitud(contrasena)?;

    let mut llave = Zeroizing::new([0; BYTES_LLAVE]);
    aleatorio(llave.as_mut_slice())?;
    let mut secreto = Zeroizing::new([0; BYTES_CODIGO]);
    aleatorio(secreto.as_mut_slice())?;

    let boveda = Boveda {
        version: VERSION,
        argon2,
        contrasena: envolver(&llave, contrasena.as_bytes(), argon2)?,
        codigo: envolver(&llave, secreto.as_slice(), argon2)?,
    };
    Ok(Nueva {
        llave: LlaveMaestra(llave),
        codigo: formatear_codigo(secreto.as_slice()),
        boveda,
    })
}

/// Devuelve la llave si la contraseña es la buena.
pub fn abrir(home: &Path, contrasena: &str) -> Result<LlaveMaestra, ErrorBoveda> {
    let boveda = leer(home)?;
    desenvolver(&boveda.contrasena, contrasena.as_bytes(), boveda.argon2)
}

/// Pone una contraseña nueva con el código. El código no cambia: sigue valiendo para la próxima.
pub fn recuperar(home: &Path, codigo: &str, nueva: &str) -> Result<LlaveMaestra, ErrorBoveda> {
    let mut boveda = leer(home)?;
    comprobar_longitud(nueva)?;
    let secreto = interpretar_codigo(codigo).ok_or(ErrorBoveda::Incorrecta)?;
    let llave = desenvolver(&boveda.codigo, secreto.as_slice(), boveda.argon2)?;
    boveda.contrasena = envolver(&llave.0, nueva.as_bytes(), boveda.argon2)?;
    escribir(home, &boveda, reemplazar)?;
    Ok(llave)
}

/// Solo reescribe la envoltura de la contraseña; la tablilla no se entera.
pub fn cambiar_contrasena(home: &Path, actual: &str, nueva: &str) -> Result<(), ErrorBoveda> {
    let mut boveda = leer(home)?;
    comprobar_longitud(nueva)?;
    let llave = desenvolver(&boveda.contrasena, actual.as_bytes(), boveda.argon2)?;
    boveda.contrasena = envolver(&llave.0, nueva.as_bytes(), boveda.argon2)?;
    escribir(home, &boveda, reemplazar)
}

// Caracteres y no bytes: con `len()`, "ñ" contaría doble y pasarían contraseñas más cortas.
fn comprobar_longitud(contrasena: &str) -> Result<(), ErrorBoveda> {
    if contrasena.chars().count() < CONTRASENA_MINIMA {
        return Err(ErrorBoveda::ContrasenaCorta);
    }
    Ok(())
}

fn aleatorio(destino: &mut [u8]) -> Result<(), ErrorBoveda> {
    getrandom::fill(destino).map_err(|e| ErrorBoveda::Io(format!("sin azar del sistema: {e}")))
}

fn derivar(
    secreto: &[u8],
    sal: &[u8; BYTES_SAL],
    argon2: Parametros,
) -> Result<Aes256Gcm, ErrorBoveda> {
    let mut derivada = Zeroizing::new([0; BYTES_LLAVE]);
    argon2
        .argon2()?
        .hash_password_into(secreto, sal, derivada.as_mut_slice())
        // Con parámetros y sal ya validados solo queda quedarse sin memoria. Nunca `Incorrecta`:
        // haría creer al usuario que olvidó la contraseña.
        .map_err(|e| ErrorBoveda::Io(format!("Argon2 falló: {e}")))?;
    Aes256Gcm::new_from_slice(derivada.as_slice())
        .map_err(|_| ErrorBoveda::Io("llave derivada de tamaño inválido".to_string()))
}

fn envolver(
    llave: &[u8; BYTES_LLAVE],
    secreto: &[u8],
    argon2: Parametros,
) -> Result<Envoltura, ErrorBoveda> {
    // Sal y nonce nuevos en cada envoltura: repetir un nonce con la misma llave rompe GCM.
    let mut sal = [0; BYTES_SAL];
    aleatorio(&mut sal)?;
    let mut nonce = [0; BYTES_NONCE];
    aleatorio(&mut nonce)?;
    let cifrado = derivar(secreto, &sal, argon2)?
        .encrypt((&nonce).into(), llave.as_slice())
        .map_err(|_| ErrorBoveda::Io("no pude cifrar la llave".to_string()))?;
    Ok(Envoltura {
        sal: BASE64.encode(&sal),
        nonce: BASE64.encode(&nonce),
        cifrado: BASE64.encode(&cifrado),
    })
}

fn desenvolver(
    envoltura: &Envoltura,
    secreto: &[u8],
    argon2: Parametros,
) -> Result<LlaveMaestra, ErrorBoveda> {
    let (sal, nonce, cifrado) = envoltura.partes()?;
    // La etiqueta de GCM es lo que delata el secreto equivocado: sin ella saldrían 32 bytes
    // de basura y SQLCipher fallaría después con un error incomprensible.
    let claro = Zeroizing::new(
        derivar(secreto, &sal, argon2)?
            .decrypt((&nonce).into(), cifrado.as_slice())
            .map_err(|_| ErrorBoveda::Incorrecta)?,
    );
    let mut llave = Zeroizing::new([0; BYTES_LLAVE]);
    if claro.len() != BYTES_LLAVE {
        return Err(danada("llave de tamaño inesperado"));
    }
    llave.copy_from_slice(&claro);
    Ok(LlaveMaestra(llave))
}

fn formatear_codigo(secreto: &[u8]) -> Zeroizing<String> {
    let plano = Zeroizing::new(BASE32.encode(secreto));
    let mut grupos = String::with_capacity(plano.len() + plano.len() / CARACTERES_POR_GRUPO);
    for (i, c) in plano.chars().enumerate() {
        if i > 0 && i % CARACTERES_POR_GRUPO == 0 {
            grupos.push('-');
        }
        grupos.push(c);
    }
    Zeroizing::new(grupos)
}

/// Quien copia el código a mano no debe fallar por un guion o una minúscula. Lo que tiene forma
/// imposible es simplemente un código incorrecto.
fn interpretar_codigo(codigo: &str) -> Option<Zeroizing<Vec<u8>>> {
    let mut plano = Zeroizing::new(String::with_capacity(codigo.len()));
    for c in codigo.chars() {
        if c != '-' && !c.is_whitespace() {
            plano.push(c.to_ascii_uppercase());
        }
    }
    let secreto = Zeroizing::new(BASE32.decode(plano.as_bytes()).ok()?);
    (secreto.len() == BYTES_CODIGO).then_some(secreto)
}

fn leer(home: &Path) -> Result<Boveda, ErrorBoveda> {
    let bytes = fs::read(home.join(BOVEDA)).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => ErrorBoveda::NoExiste,
        _ => io(e),
    })?;
    let boveda: Boveda =
        serde_json::from_slice(&bytes).map_err(|_| danada("no es una bóveda válida"))?;
    if boveda.version != VERSION {
        return Err(ErrorBoveda::Danada(format!(
            "versión {} desconocida",
            boveda.version
        )));
    }
    boveda.argon2.argon2()?;
    // También la envoltura que no se va a usar: mejor saberlo ahora que el día que haga falta.
    boveda.contrasena.partes()?;
    boveda.codigo.partes()?;
    Ok(boveda)
}

/// Escribe en un temporal y lo coloca de un solo golpe: si se va la luz a medias, `boveda.json`
/// sigue entero. Perderlo sería perder el archivo. El temporal va en `home` porque `rename`
/// solo es atómico dentro de un mismo sistema de archivos.
fn escribir(
    home: &Path,
    boveda: &Boveda,
    colocar: fn(&Path, &Path) -> Result<(), ErrorBoveda>,
) -> Result<(), ErrorBoveda> {
    let json = serde_json::to_vec_pretty(boveda).map_err(|e| ErrorBoveda::Io(e.to_string()))?;
    let temporal = ruta_temporal(home)?;
    let destino = home.join(BOVEDA);
    let resultado = volcar(&temporal, &json).and_then(|()| colocar(&temporal, &destino));
    // Si se colocó con `rename` ya no está y esto falla sin importar.
    let _ = fs::remove_file(&temporal);
    resultado?;
    sincronizar_carpeta(home);
    Ok(())
}

// Nombre al azar para que dos procesos no compartan temporal y uno publique lo que el otro
// escribe a medias. Por eso también uno abandonado no estorba nunca.
fn ruta_temporal(home: &Path) -> Result<PathBuf, ErrorBoveda> {
    let mut azar = [0; 8];
    aleatorio(&mut azar)?;
    Ok(home.join(format!("{BOVEDA}.{}.tmp", HEXLOWER.encode(&azar))))
}

fn volcar(ruta: &Path, contenido: &[u8]) -> Result<(), ErrorBoveda> {
    let mut opciones = fs::OpenOptions::new();
    opciones.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opciones, 0o600);
    let mut archivo = opciones.open(ruta).map_err(io)?;
    archivo.write_all(contenido).map_err(io)?;
    // Sin esto, tras un corte el `rename` puede sobrevivir y el contenido no.
    archivo.sync_all().map_err(io)
}

fn reemplazar(temporal: &Path, destino: &Path) -> Result<(), ErrorBoveda> {
    fs::rename(temporal, destino).map_err(io)
}

/// Como `rename`, pero sin pisar. `rename` sustituye al destino, así que dos creaciones a la vez
/// dejarían viva solo la segunda llave aunque la primera ya hubiese escrito su tablilla.
/// `hard_link` falla si el destino existe, y esa comprobación es atómica.
fn colocar_sin_pisar(temporal: &Path, destino: &Path) -> Result<(), ErrorBoveda> {
    match fs::hard_link(temporal, destino) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(ErrorBoveda::YaExiste),
        // FAT y compañía no tienen enlaces duros: queda la comprobación previa de `guardar`.
        Err(_) => reemplazar(temporal, destino),
    }
}

/// `rename` queda en la carpeta, que también hay que asentar en disco. A mejor esfuerzo: si
/// falla, el cambio ya está hecho y avisar de error sería mentir.
fn sincronizar_carpeta(home: &Path) {
    #[cfg(unix)]
    if let Ok(carpeta) = fs::File::open(home) {
        let _ = carpeta.sync_all();
    }
    #[cfg(not(unix))]
    let _ = home;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{tempdir, TempDir};

    const CONTRASENA: &str = "correcta caballo pila";

    // Argon2 real tarda ~1 s; aquí bastan los parámetros mínimos porque se prueba el flujo.
    const BARATOS: Parametros = Parametros {
        m_kib: 8,
        t: 1,
        p: 1,
    };

    fn nueva(contrasena: &str) -> Nueva {
        preparar_con(contrasena, BARATOS).unwrap()
    }

    /// Bóveda ya guardada: carpeta, llave y código.
    fn creada() -> (TempDir, LlaveMaestra, String) {
        let d = tempdir().unwrap();
        let n = nueva(CONTRASENA);
        let codigo = n.codigo().to_string();
        let llave = n.guardar(d.path()).unwrap();
        (d, llave, codigo)
    }

    fn error<T>(r: Result<T, ErrorBoveda>) -> ErrorBoveda {
        r.err().expect("debía fallar")
    }

    fn json(home: &Path) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(home.join(BOVEDA)).unwrap()).unwrap()
    }

    fn escribir_json(home: &Path, v: &serde_json::Value) {
        fs::write(home.join(BOVEDA), v.to_string()).unwrap();
    }

    #[test]
    fn crear_y_abrir_devuelve_la_misma_llave() {
        let (d, llave, _) = creada();
        assert_eq!(*abrir(d.path(), CONTRASENA).unwrap().hex(), *llave.hex());
    }

    #[test]
    fn dos_bovedas_tienen_llaves_distintas() {
        let (_d1, a, _) = creada();
        let (_d2, b, _) = creada();
        assert_ne!(*a.hex(), *b.hex());
    }

    #[test]
    fn hex_son_64_digitos_en_minuscula() {
        let (_d, llave, _) = creada();
        let hex = llave.hex();
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    }

    #[test]
    fn contrasena_incorrecta_da_incorrecta() {
        let (d, _, _) = creada();
        assert_eq!(
            error(abrir(d.path(), "otra contraseña larga")),
            ErrorBoveda::Incorrecta
        );
    }

    #[test]
    fn contrasena_corta_se_rechaza() {
        // Cuenta caracteres, no bytes: "ñ" ocupa dos y no debe regalar longitud.
        assert_eq!(
            error(preparar_con("ñandúñandúñ", BARATOS)),
            ErrorBoveda::ContrasenaCorta
        );
        assert_eq!(
            error(preparar_con("12345678901", BARATOS)),
            ErrorBoveda::ContrasenaCorta
        );
        assert!(preparar_con("123456789012", BARATOS).is_ok());
        assert!(preparar_con("ñandúñandúñu", BARATOS).is_ok());
        assert_eq!(error(preparar("12345678901")), ErrorBoveda::ContrasenaCorta);

        let (d, _, codigo) = creada();
        assert_eq!(
            error(recuperar(d.path(), &codigo, "12345678901")),
            ErrorBoveda::ContrasenaCorta
        );
        assert_eq!(
            error(cambiar_contrasena(d.path(), CONTRASENA, "12345678901")),
            ErrorBoveda::ContrasenaCorta
        );
        assert!(abrir(d.path(), CONTRASENA).is_ok());
    }

    #[test]
    fn sin_boveda_da_no_existe() {
        let d = tempdir().unwrap();
        let larga = "contraseña bien larga";
        assert_eq!(error(abrir(d.path(), larga)), ErrorBoveda::NoExiste);
        assert_eq!(
            error(recuperar(d.path(), "AAAA", larga)),
            ErrorBoveda::NoExiste
        );
        assert_eq!(
            error(cambiar_contrasena(d.path(), larga, larga)),
            ErrorBoveda::NoExiste
        );
        // Aunque la contraseña nueva sea corta: lo primero es que no hay bóveda.
        assert_eq!(
            error(recuperar(d.path(), "AAAA", "corta")),
            ErrorBoveda::NoExiste
        );
    }

    #[test]
    fn codigo_recupera_y_la_contrasena_vieja_deja_de_valer() {
        let (d, llave, codigo) = creada();
        let nueva = "contraseña nueva tras perderla";
        let recuperada = recuperar(d.path(), &codigo, nueva).unwrap();
        assert_eq!(*recuperada.hex(), *llave.hex());
        assert_eq!(*abrir(d.path(), nueva).unwrap().hex(), *llave.hex());
        assert_eq!(error(abrir(d.path(), CONTRASENA)), ErrorBoveda::Incorrecta);
    }

    #[test]
    fn codigo_sigue_valiendo_tras_recuperar() {
        let (d, llave, codigo) = creada();
        recuperar(d.path(), &codigo, "primera contraseña nueva").unwrap();
        let otra = recuperar(d.path(), &codigo, "segunda contraseña nueva").unwrap();
        assert_eq!(*otra.hex(), *llave.hex());
    }

    #[test]
    fn recuperar_solo_cambia_la_envoltura_de_la_contrasena() {
        let (d, _, codigo) = creada();
        let antes = json(d.path());
        recuperar(d.path(), &codigo, "contraseña nueva tras perderla").unwrap();
        let despues = json(d.path());
        assert_eq!(antes["codigo"], despues["codigo"]);
        assert_ne!(antes["contrasena"], despues["contrasena"]);
    }

    #[test]
    fn codigo_acepta_minusculas_y_sin_separadores() {
        let (d, llave, codigo) = creada();
        let plano: String = codigo.chars().filter(|c| *c != '-').collect();
        let variantes = [
            plano.clone(),
            plano.to_lowercase(),
            codigo.to_lowercase(),
            codigo.replace('-', " "),
            format!("  {codigo}\n"),
        ];
        for v in variantes {
            let r = recuperar(d.path(), &v, "contraseña nueva tras perderla").unwrap();
            assert_eq!(*r.hex(), *llave.hex(), "{v}");
        }
    }

    #[test]
    fn codigo_malo_da_incorrecta_y_no_toca_nada() {
        let (d, _, codigo) = creada();
        let antes = fs::read(d.path().join(BOVEDA)).unwrap();
        let primera = if codigo.starts_with('A') { 'B' } else { 'A' };
        let cambiado = format!("{primera}{}", &codigo[1..]);
        for malo in [cambiado.as_str(), "", "AAAA", "no es base32 !!", "ÑÑÑÑ"] {
            assert_eq!(
                error(recuperar(d.path(), malo, "contraseña nueva larga")),
                ErrorBoveda::Incorrecta,
                "{malo}"
            );
        }
        assert_eq!(fs::read(d.path().join(BOVEDA)).unwrap(), antes);
    }

    #[test]
    fn cambiar_contrasena_conserva_la_llave() {
        let (d, llave, codigo) = creada();
        let nueva = "contraseña recién cambiada";
        let antes = json(d.path());
        cambiar_contrasena(d.path(), CONTRASENA, nueva).unwrap();
        assert_eq!(*abrir(d.path(), nueva).unwrap().hex(), *llave.hex());
        assert_eq!(error(abrir(d.path(), CONTRASENA)), ErrorBoveda::Incorrecta);
        let despues = json(d.path());
        assert_eq!(antes["codigo"], despues["codigo"]);
        assert_eq!(antes["argon2"], despues["argon2"]);
        assert_ne!(antes["contrasena"]["sal"], despues["contrasena"]["sal"]);
        assert_ne!(antes["contrasena"]["nonce"], despues["contrasena"]["nonce"]);
        assert_eq!(
            *recuperar(d.path(), &codigo, "otra contraseña larga")
                .unwrap()
                .hex(),
            *llave.hex()
        );
    }

    #[test]
    fn cambiar_con_actual_mala_no_toca_nada() {
        let (d, _, _) = creada();
        let antes = fs::read(d.path().join(BOVEDA)).unwrap();
        assert_eq!(
            error(cambiar_contrasena(
                d.path(),
                "no es la contraseña",
                "contraseña nueva larga"
            )),
            ErrorBoveda::Incorrecta
        );
        assert_eq!(fs::read(d.path().join(BOVEDA)).unwrap(), antes);
    }

    #[test]
    fn codigo_tiene_32_caracteres_base32() {
        let n = nueva(CONTRASENA);
        let limpio: String = n.codigo().chars().filter(|c| *c != '-').collect();
        assert_eq!(limpio.len(), 32);
        assert!(limpio.chars().all(|c| matches!(c, 'A'..='Z' | '2'..='7')));
        assert_eq!(BASE32.decode(limpio.as_bytes()).unwrap().len(), 20);
        assert!(
            n.codigo().split('-').all(|g| g.len() == 4),
            "{}",
            n.codigo()
        );
    }

    #[test]
    fn el_archivo_no_contiene_llave_ni_contrasena() {
        let (d, llave, codigo) = creada();
        let crudo = fs::read_to_string(d.path().join(BOVEDA)).unwrap();
        let hex = llave.hex();
        let plano: String = codigo.chars().filter(|c| *c != '-').collect();
        let bytes = HEXLOWER.decode(hex.as_bytes()).unwrap();
        for secreto in [
            CONTRASENA.to_string(),
            codigo.clone(),
            plano,
            hex.to_string(),
            hex.to_uppercase(),
            BASE64.encode(&bytes),
        ] {
            assert!(!crudo.contains(&secreto), "fuga: {secreto}");
        }
    }

    #[test]
    fn el_archivo_lleva_version_parametros_y_envolturas() {
        let (d, _, _) = creada();
        let v = json(d.path());
        assert_eq!(v["version"], 1);
        assert_eq!(v["argon2"]["m_kib"], BARATOS.m_kib);
        assert_eq!(v["argon2"]["t"], BARATOS.t);
        assert_eq!(v["argon2"]["p"], BARATOS.p);
        for envoltura in ["contrasena", "codigo"] {
            let bytes = |campo: &str| {
                BASE64
                    .decode(v[envoltura][campo].as_str().unwrap().as_bytes())
                    .unwrap()
                    .len()
            };
            assert_eq!(bytes("sal"), 16);
            assert_eq!(bytes("nonce"), 12);
            assert_eq!(bytes("cifrado"), 48);
        }
        assert_ne!(v["contrasena"]["sal"], v["codigo"]["sal"]);
    }

    #[test]
    fn parametros_de_produccion_son_los_calibrados() {
        assert_eq!(PARAMETROS.m_kib, 128 * 1024);
        assert_eq!(PARAMETROS.t, 14);
        assert_eq!(PARAMETROS.p, 1);
    }

    #[test]
    fn contrasena_con_espacios_y_enye_se_conserva() {
        let con_espacios = " ñandú 🦤 largo ";
        let d = tempdir().unwrap();
        let llave = nueva(con_espacios).guardar(d.path()).unwrap();
        assert_eq!(*abrir(d.path(), con_espacios).unwrap().hex(), *llave.hex());
        assert_eq!(
            error(abrir(d.path(), con_espacios.trim_end())),
            ErrorBoveda::Incorrecta
        );
        assert_eq!(
            error(abrir(d.path(), con_espacios.trim())),
            ErrorBoveda::Incorrecta
        );
    }

    #[test]
    fn boveda_danada_da_error_claro() {
        let (d, _, codigo) = creada();
        let bien = json(d.path());
        let danada = |v: serde_json::Value| {
            escribir_json(d.path(), &v);
            assert!(
                matches!(error(abrir(d.path(), CONTRASENA)), ErrorBoveda::Danada(_)),
                "{v}"
            );
            assert!(matches!(
                error(recuperar(d.path(), &codigo, CONTRASENA)),
                ErrorBoveda::Danada(_)
            ));
            assert!(matches!(
                error(cambiar_contrasena(d.path(), CONTRASENA, CONTRASENA)),
                ErrorBoveda::Danada(_)
            ));
        };

        let mut v = bien.clone();
        v["version"] = 2.into();
        danada(v);

        let mut v = bien.clone();
        v.as_object_mut().unwrap().remove("version");
        danada(v);

        let mut v = bien.clone();
        v.as_object_mut().unwrap().remove("codigo");
        danada(v);

        let mut v = bien.clone();
        v["contrasena"]["cifrado"] = "esto no es base64 ***".into();
        danada(v);

        // Longitudes imposibles, también en la envoltura que no se está usando.
        let mut v = bien.clone();
        v["contrasena"]["sal"] = BASE64.encode(&[0; 15]).into();
        danada(v);

        let mut v = bien.clone();
        v["codigo"]["nonce"] = BASE64.encode(&[0; 11]).into();
        danada(v);

        let mut v = bien.clone();
        v["codigo"]["cifrado"] = BASE64.encode(&[0; 47]).into();
        danada(v);

        let mut v = bien.clone();
        v["argon2"]["t"] = 0.into();
        danada(v);

        // Sin tope, un archivo manipulado reservaría memoria hasta tumbar el proceso.
        let mut v = bien.clone();
        v["argon2"]["m_kib"] = u32::MAX.into();
        danada(v);

        let mut v = bien.clone();
        v["argon2"]["t"] = u32::MAX.into();
        danada(v);

        for basura in ["", "no soy json", "[]", "{}"] {
            fs::write(d.path().join(BOVEDA), basura).unwrap();
            assert!(
                matches!(error(abrir(d.path(), CONTRASENA)), ErrorBoveda::Danada(_)),
                "{basura:?}"
            );
        }

        fs::write(d.path().join(BOVEDA), [0xff, 0xfe, 0x00]).unwrap();
        assert!(matches!(
            error(abrir(d.path(), CONTRASENA)),
            ErrorBoveda::Danada(_)
        ));
    }

    #[test]
    fn cifrado_alterado_da_incorrecta() {
        // GCM no distingue un bit cambiado de una llave mala: ambos fallan la etiqueta.
        let (d, _, _) = creada();
        let mut v = json(d.path());
        let mut cifrado = BASE64
            .decode(v["contrasena"]["cifrado"].as_str().unwrap().as_bytes())
            .unwrap();
        cifrado[0] ^= 1;
        v["contrasena"]["cifrado"] = BASE64.encode(&cifrado).into();
        escribir_json(d.path(), &v);
        assert_eq!(error(abrir(d.path(), CONTRASENA)), ErrorBoveda::Incorrecta);
    }

    #[test]
    fn guardar_se_niega_si_existe_boveda_o_tablilla() {
        let (d, llave, _) = creada();
        let antes = fs::read(d.path().join(BOVEDA)).unwrap();
        assert_eq!(
            error(nueva("otra contraseña larga").guardar(d.path())),
            ErrorBoveda::YaExiste
        );
        assert_eq!(fs::read(d.path().join(BOVEDA)).unwrap(), antes);
        assert_eq!(*abrir(d.path(), CONTRASENA).unwrap().hex(), *llave.hex());

        // Una tablilla sin bóveda es un archivo que otra llave ya no podría abrir.
        for restos in [
            TABLILLA_WEB.to_string(),
            format!("{TABLILLA_WEB}-wal"),
            format!("{TABLILLA_WEB}-shm"),
        ] {
            let d = tempdir().unwrap();
            fs::write(d.path().join(&restos), "x").unwrap();
            assert_eq!(
                error(nueva(CONTRASENA).guardar(d.path())),
                ErrorBoveda::YaExiste,
                "{restos}"
            );
            assert!(!d.path().join(BOVEDA).exists());
        }
    }

    #[test]
    fn colocar_sin_pisar_respeta_lo_que_haya_aparecido() {
        // Simula a otro proceso creando la bóveda justo después de la comprobación de `guardar`.
        let d = tempdir().unwrap();
        let (temporal, destino) = (d.path().join("t"), d.path().join(BOVEDA));
        fs::write(&temporal, "mía").unwrap();
        fs::write(&destino, "del otro proceso").unwrap();
        assert_eq!(
            colocar_sin_pisar(&temporal, &destino),
            Err(ErrorBoveda::YaExiste)
        );
        assert_eq!(fs::read_to_string(&destino).unwrap(), "del otro proceso");
    }

    #[test]
    fn preparar_no_escribe_nada_hasta_guardar() {
        let d = tempdir().unwrap();
        let n = nueva(CONTRASENA);
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
        drop(n);
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
    }

    #[test]
    fn no_quedan_temporales_tras_escribir() {
        let (d, _, codigo) = creada();
        cambiar_contrasena(d.path(), CONTRASENA, "contraseña intermedia").unwrap();
        recuperar(d.path(), &codigo, "contraseña definitiva").unwrap();
        let nombres: Vec<_> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(nombres, [BOVEDA]);
    }

    #[test]
    fn temporal_abandonado_no_molesta() {
        // Un corte de luz entre escribir el temporal y el `rename` deja esto atrás.
        let d = tempdir().unwrap();
        for resto in ["boveda.json.tmp", "boveda.json.00112233.tmp"] {
            fs::write(d.path().join(resto), "a medias").unwrap();
        }
        assert_eq!(error(abrir(d.path(), CONTRASENA)), ErrorBoveda::NoExiste);

        let n = nueva(CONTRASENA);
        let codigo = n.codigo().to_string();
        let llave = n.guardar(d.path()).unwrap();
        assert_eq!(*abrir(d.path(), CONTRASENA).unwrap().hex(), *llave.hex());

        let nueva_c = "contraseña tras el corte";
        cambiar_contrasena(d.path(), CONTRASENA, nueva_c).unwrap();
        assert_eq!(*abrir(d.path(), nueva_c).unwrap().hex(), *llave.hex());
        recuperar(d.path(), &codigo, CONTRASENA).unwrap();
        assert_eq!(*abrir(d.path(), CONTRASENA).unwrap().hex(), *llave.hex());
    }

    #[cfg(unix)]
    #[test]
    fn boveda_solo_la_lee_su_dueno() {
        use std::os::unix::fs::PermissionsExt;
        let (d, _, _) = creada();
        let modo = |d: &Path| fs::metadata(d.join(BOVEDA)).unwrap().permissions().mode();
        assert_eq!(modo(d.path()) & 0o777, 0o600);
        cambiar_contrasena(d.path(), CONTRASENA, "contraseña recién cambiada").unwrap();
        assert_eq!(modo(d.path()) & 0o777, 0o600);
    }

    #[test]
    fn debug_y_errores_no_muestran_secretos() {
        let d = tempdir().unwrap();
        let n = nueva(CONTRASENA);
        let codigo = n.codigo().to_string();
        let visto = format!("{n:?}");
        assert!(!visto.contains(&codigo), "{visto}");
        let llave = n.guardar(d.path()).unwrap();
        let hex = llave.hex();
        let visto = format!("{llave:?}");
        assert!(!visto.contains(hex.as_str()), "{visto}");

        for e in [
            ErrorBoveda::NoExiste,
            ErrorBoveda::YaExiste,
            ErrorBoveda::Incorrecta,
            ErrorBoveda::ContrasenaCorta,
            ErrorBoveda::Danada("x".into()),
            ErrorBoveda::Io("y".into()),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }

    /// Mide una apertura real. En release:
    /// `cargo test --release -p nisaba-core calibrar -- --ignored --nocapture`.
    #[test]
    #[ignore = "lento: Argon2 con los parámetros de verdad"]
    fn calibrar_apertura() {
        let d = tempdir().unwrap();
        preparar(CONTRASENA).unwrap().guardar(d.path()).unwrap();
        for _ in 0..3 {
            let t = std::time::Instant::now();
            abrir(d.path(), CONTRASENA).unwrap();
            println!("apertura: {:?}", t.elapsed());
        }
    }
}
