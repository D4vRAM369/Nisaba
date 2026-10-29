#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fuente {
    ClaudeCode,
    Codex,
}

impl Fuente {
    pub fn as_str(&self) -> &'static str {
        match self {
            Fuente::ClaudeCode => "claude-code",
            Fuente::Codex => "codex",
        }
    }

    pub fn prefijo(&self) -> &'static str {
        match self {
            Fuente::ClaudeCode => "cc",
            Fuente::Codex => "cx",
        }
    }

    pub fn desde_str(s: &str) -> Option<Fuente> {
        match s {
            "claude-code" => Some(Fuente::ClaudeCode),
            "codex" => Some(Fuente::Codex),
            _ => None,
        }
    }
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

impl serde::Serialize for Fuente {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}
