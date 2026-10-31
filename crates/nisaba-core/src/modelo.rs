#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fuente {
    ClaudeCode,
    Codex,
    ClaudeWeb,
    ChatGpt,
}

impl Fuente {
    pub const TODAS: [Fuente; 4] = [
        Fuente::ClaudeCode,
        Fuente::Codex,
        Fuente::ClaudeWeb,
        Fuente::ChatGpt,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Fuente::ClaudeCode => "claude-code",
            Fuente::Codex => "codex",
            Fuente::ClaudeWeb => "claude-web",
            Fuente::ChatGpt => "chatgpt",
        }
    }

    pub fn prefijo(&self) -> &'static str {
        match self {
            Fuente::ClaudeCode => "cc",
            Fuente::Codex => "cx",
            Fuente::ClaudeWeb => "cw",
            Fuente::ChatGpt => "gpt",
        }
    }

    pub fn desde_str(s: &str) -> Option<Fuente> {
        match s {
            "claude-code" => Some(Fuente::ClaudeCode),
            "codex" => Some(Fuente::Codex),
            "claude-web" => Some(Fuente::ClaudeWeb),
            "chatgpt" => Some(Fuente::ChatGpt),
            _ => None,
        }
    }

    /// Viene de una exportación web y vive en la tablilla cifrada.
    pub fn es_web(&self) -> bool {
        matches!(self, Fuente::ClaudeWeb | Fuente::ChatGpt)
    }
}

/// `cw-…` o `gpt-…`: la conversación vive en la tablilla cifrada.
pub fn es_id_web(id_publico: &str) -> bool {
    id_publico.split_once('-').is_some_and(|(prefijo, _)| {
        Fuente::TODAS
            .iter()
            .any(|f| f.es_web() && f.prefijo() == prefijo)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rol {
    Usuario,
    Asistente,
    Sistema,
}

impl Rol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Rol::Usuario => "usuario",
            Rol::Asistente => "asistente",
            Rol::Sistema => "sistema",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoParte {
    Texto,
    Razonamiento,
    Herramienta,
    Sistema,
    Imagen,
}

impl TipoParte {
    pub fn as_str(&self) -> &'static str {
        match self {
            TipoParte::Texto => "texto",
            TipoParte::Razonamiento => "razonamiento",
            TipoParte::Herramienta => "herramienta",
            TipoParte::Sistema => "sistema",
            TipoParte::Imagen => "imagen",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parte {
    pub tipo: TipoParte,
    pub contenido: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mensaje {
    pub uuid: Option<String>,
    pub id_padre: Option<String>,
    pub rol: Rol,
    pub modelo: Option<String>,
    pub hora: Option<String>,
    pub oculto: bool,
    pub partes: Vec<Parte>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversacionParseada {
    pub fuente: Fuente,
    pub id_original: String,
    pub cwd: Option<String>,
    /// Solo subagentes.
    pub padre_original: Option<String>,
    /// Solo subagentes.
    pub descripcion: Option<String>,
    /// uuid del último mensaje de la rama activa, si la fuente lo indica.
    pub hoja: Option<String>,
    pub mensajes: Vec<Mensaje>,
    pub avisos: Vec<String>,
}

/// Qué es una fila web: un chat, un documento (de proyecto, memoria o frame) o un chat de diseño.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoWeb {
    Conversacion,
    Documento,
    Diseno,
}

impl TipoWeb {
    pub fn as_str(&self) -> &'static str {
        match self {
            TipoWeb::Conversacion => "conversacion",
            TipoWeb::Documento => "documento",
            TipoWeb::Diseno => "diseno",
        }
    }
}

/// Una conversación de una exportación web, ya normalizada.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversacionImportada {
    /// `cwd` a `None` y sin padre: en la web no hay carpeta ni subagentes.
    pub base: ConversacionParseada,
    pub titulo: Option<String>,
    pub proyecto: Option<String>,
    pub inicio: Option<String>,
    /// Última actualización de la conversación según la exportación (ISO 8601), no la hora del
    /// último mensaje: decide si lo que llega es un ZIP viejo. Si no se conoce, `None`.
    pub fin: Option<String>,
    pub tipo: TipoWeb,
    /// El JSON tal cual llegó, para interpretar en el futuro lo que hoy no se lee.
    pub original: Option<String>,
}

impl serde::Serialize for Fuente {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}
