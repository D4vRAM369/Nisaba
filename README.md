<div align="center">

<img src="banner.jpg" alt="NISABA: la diosa sumeria de la escritura graba una tablilla de arcilla entre espigas, en oro sobre lapislázuli" width="100%">

**La bóveda local de tus conversaciones con IAs.**
Encuentra por palabra o frase lo que hablaste con Claude Code, Codex o Claude.ai, y reléelo sin ruido.

[English](README-EN.md) · Licencia [GPL-3.0](LICENSE) · v0.1.2

> ⚠️ **Versión temprana (0.x).** Funciona y se usa a diario, pero aún no es estable: pueden cambiar comandos y formatos.

![Nisaba: buscador a la izquierda y lector «solo conversación» a la derecha](assets/captura-lector.png)

</div>

## ¿Por qué este proyecto?

Trabajo a diario con varias IAs. Lo que aprendí y decidí con ellas queda repartido en formatos y carpetas distintas, y una parte se pierde: **Claude Code borra por defecto las sesiones de más de 30 días**. Quería tener un lugar donde poder acceder a mis conversaciones sobre mis proyectos con mis IA frecuentes en mi propio PC, sin depender de las webs oficiales.

Nisaba lee las sesiones que esas herramientas ya guardan en tu disco (y las exportaciones ZIP que descargues de la web), las indexa y te deja **buscarlas y releerlas bien presentadas**: tus mensajes, las respuestas, el código y el Markdown, sin muros de llamadas a herramientas. Si una fuente borra una sesión, la tablilla la conserva: **la tablilla solo crece**.

El nombre viene de Nisaba, la diosa sumeria de la escritura y de los escribas. La bóveda se llama "la tablilla".

## Qué hace hoy (v0.1.2)

- **Fuentes locales:** Claude Code (`~/.claude/projects`) y Codex (`~/.codex`).
- **Archivo web cifrado (Claude.ai):** importa la carpeta de ZIPs o el `.zip` oficial de Claude.ai (conversaciones, proyectos y sus documentos, memorias, *frames* HTML y chats de diseño; `light_metadata` nunca se lee) en una segunda base cifrada con **SQLCipher** (`tablilla-web.db`), sin extraer archivos temporales al disco.
- **Búsqueda** por palabra, frase entre comillas o prefijo (`medica*`), con filtros de fuente, proyecto y fecha. Cuando el archivo web está desbloqueado, mezcla resultados locales y web.
- **Lector "solo conversación" + panel Detalles:** las herramientas se resumen en una línea y el razonamiento queda plegado. En conversaciones de Claude.ai (`cw-…`), un panel lateral muestra el proyecto y sus documentos, adjuntos (texto extraído), fuentes citadas, herramientas, ramas editadas, versiones anteriores y vista previa de *frames* HTML en un `iframe` con `sandbox` vacío.
- **Panel de salud:** archivos vistos frente a parseados por fuente, para que nunca falte nada sin que lo sepas.
- **Exportar** una conversación completa a `.md` o `.pdf`, con aviso si parece llevar una clave o un token (y recordatorio de que el archivo exportado queda sin cifrar).
- **Volver al origen:** copia el comando para reanudar la sesión local en Claude Code o Codex, o abre la carpeta del proyecto.
- **No indexar:** excluye carpetas antes de que se lean; lo ya guardado se borra solo si lo confirmas.
- **Tres puertas:** la ventana, la terminal (`nisaba`) y tus IAs vía **MCP** (el servidor MCP solo accede a la tablilla local y nunca abre el archivo web cifrado).
- **Sin red:** nada sale de tu equipo.

## Instalación

Descarga el archivo de tu sistema desde [Releases](https://github.com/D4vRAM369/Nisaba/releases). Todos los paquetes incluyen la ventana (`nisaba-gui`) **y** la terminal (`nisaba`).

### Linux

| Formato | Instalar |
|---|---|
| `.deb` (Debian, Ubuntu, Mint…) | `sudo apt install ./Nisaba_0.1.2_amd64.deb` |
| `.rpm` (Fedora, openSUSE…) | `sudo dnf install ./Nisaba-0.1.2-1.x86_64.rpm` |
| AppImage (portable, sin instalar) | `chmod +x Nisaba_0.1.2_amd64.AppImage && ./Nisaba_0.1.2_amd64.AppImage` |
| Flatpak | `flatpak install --user ./Nisaba_0.1.2_x86_64.flatpak` (necesita el runtime `org.gnome.Platform//50` de Flathub) |

El Flatpak solo puede **leer** `~/.claude` y `~/.codex`, y no tiene acceso a la red.

### Windows 10/11

| Formato | Instalar |
|---|---|
| Instalador (`Nisaba_0.1.2_x64-setup.exe`) | Doble clic. Añade `nisaba` al `Path` de tu usuario. |
| `.msi` | Doble clic (pensado para despliegues gestionados). |
| Portable (`Nisaba_0.1.2_x64_portable.zip`) | Descomprime y abre `nisaba-gui.exe`. El índice se guarda en `datos\`, junto al `.exe`. |

Los binarios aún no están firmados, así que Windows mostrará **"Windows protegió su PC"**. Pulsa **Más información → Ejecutar de todas formas**.

## ¿Cómo usarlo?

### En la ventana

1. Abre **Nisaba**. La primera vez indexa tus sesiones (unos 30 s para ~700 conversaciones); mientras tanto ya puedes buscar.
2. Escribe en el buscador: busca mientras escribes. Usa `"una frase exacta"` o un prefijo como `medica*`.
3. Afina con **Fuente**, **Proyecto** y **Fecha**, o marca **Razonamiento** / **Código y herramientas** para buscar también ahí.
4. Pulsa un resultado: el lector salta a la coincidencia. **Ver todo** muestra cada herramienta y cada evento.
5. **Archivo web (arriba a la derecha):**
   - La primera vez, elige una contraseña (mínimo 12 caracteres), guarda el **código de recuperación de 128 bits** y confirma la casilla.
   - Una vez desbloqueado, pulsa **📁 Elegir carpeta de exportación…** (por ejemplo, la carpeta con los ZIP descargados de Claude.ai) o **📦 Elegir archivo .zip…**, o arrástralo a la ventana.
   - Al abrir una conversación de Claude.ai (`cw-…`), el panel lateral **Detalles** muestra sus documentos de proyecto, texto extraído de adjuntos (la exportación de Claude.ai no incluye los binarios originales), citas, herramientas, ramas y versiones anteriores.
   - Al cerrar la ventana, el archivo web se bloquea automáticamente.
6. En la cabecera del lector: **⤓ .md** guarda la conversación completa en Markdown y **⤓ .pdf** abre el diálogo de impresión (elige "Guardar como PDF"). Exportan lo que ves: con **Ver todo**, también herramientas y razonamiento. Si la conversación parece llevar una clave o un token, Nisaba te avisa antes.
7. **▶ Continuar** copia el comando para reanudar la sesión (`claude --resume …` desde su carpeta, o `codex resume …`); **📁 Carpeta** la abre y **⧉ Ruta** copia su ruta.
8. **Ajustes → No indexar**: añade carpetas (o elige un proyecto conocido) cuyas conversaciones no quieres en la tablilla. Si ya había algo guardado, te pregunta si borrarlo; si no, queda oculto en todas partes.
9. **Salud** (arriba a la derecha) indica si cada fuente local está completa.

### En la terminal

```bash
nisaba buscar "frase o palabra"
nisaba buscar "medica*" --fuente codex --proyecto Mirlo --desde 2026-10-01 --limite 20
nisaba buscar "algo" --json            # salida para scripts
nisaba ver cx-xxxxxxxx                  # el id lo imprime `buscar`
nisaba ver cx-xxxxxxxx --todo           # con herramientas, razonamiento y ocultos
nisaba abrir cx-xxxxxxxx                # abre esa conversación en la ventana
nisaba exportar cx-xxxxxxxx             # la guarda completa en Markdown (--todo, --salida <ruta>, --sobrescribir)
nisaba excluir ~/Proyectos/Privado      # no indexar esa carpeta (--borrar elimina lo ya guardado)
nisaba excluir --lista                  # carpetas excluidas (--quitar <carpeta> para volver a indexarla)
nisaba salud                            # estado de cada fuente
nisaba uso                              # búsquedas por día y canal

# Archivo web cifrado (Claude.ai)
nisaba importar ~/Descargas/Claude      # importa carpeta de ZIPs o un .zip individual
nisaba buscar "frase" --web             # incluye el archivo web en la búsqueda
nisaba ver cw-xxxxxxxx --version 1      # lee la conversación web o una versión anterior
nisaba contrasena                       # cambia la contraseña sin recifrar la base de datos
nisaba recuperar                        # restablece la contraseña con el código de recuperación
```

Si `exportar` encuentra posibles secretos, te los enseña enmascarados y no escribe nada hasta que añades `--forzar`. Tampoco pisa un archivo que ya exista: para reemplazarlo, usa `--sobrescribir`. Los comandos también existen en inglés (`search`, `show`, `open`, `export`, `exclude`, `index`, `health`, `usage`, `import`, `password`, `recover`, y `--source`, `--web`, `--all`, `--output`…): `nisaba --help` los lista.

En Flatpak, antepón `flatpak run --command=nisaba io.github.d4vram369.Nisaba` (por ejemplo, `flatpak run --command=nisaba io.github.d4vram369.Nisaba buscar "algo"`).

### Desde tu IA (MCP)

Nisaba incluye un servidor MCP que solo habla por `stdio` (no abre puertos). Por seguridad, el MCP solo consulta la tablilla local (`tablilla.db`) y nunca toca el archivo web cifrado. Para que Claude Code pueda buscar en tu historial local:

```bash
claude mcp add --scope user nisaba -- nisaba mcp
```

Con Flatpak:

```bash
claude mcp add --scope user nisaba -- flatpak run --command=nisaba io.github.d4vram369.Nisaba mcp
```

Después, en una sesión nueva, pídele algo como *"busca en mis conversaciones dónde decidimos usar FTS5"*.

### Carpetas y variables

| Qué | Por defecto | Variable para cambiarlo |
|---|---|---|
| Índice (`tablilla.db`, `tablilla-web.db`, `boveda.json`) | Linux: `~/.local/share/nisaba` · Windows: `%APPDATA%\nisaba` · Portable: `datos\` junto al `.exe` | `NISABA_HOME` |
| Sesiones de Claude Code | `~/.claude/projects` | `NISABA_CLAUDE_DIR` |
| Sesiones de Codex | `~/.codex` | `NISABA_CODEX_DIR` |
| Ajustes de Claude Code (aviso de limpieza) | `~/.claude/settings.json` | `NISABA_CLAUDE_SETTINGS` |

## Privacidad y cifrado del archivo web

- **Sin red, sin cuentas, sin telemetría.** Ni las fuentes tipográficas se descargan.
- Nisaba **solo lee** las carpetas de Claude Code, Codex y los ZIP que importas; nunca los modifica ni los borra.
- **Cómo se cifra el archivo web:** las conversaciones importadas y su índice FTS5 viven en `tablilla-web.db`, cifrada página a página con **SQLCipher (AES-256-CBC + HMAC-SHA512)**. La llave maestra de 256 bits es aleatoria y se guarda envuelta dos veces en `boveda.json` con **AES-256-GCM** y claves derivadas mediante **Argon2id** (una con tu contraseña y otra con tu código de recuperación de 128 bits).
- **Copia de seguridad:** guarda **siempre juntos `tablilla-web.db` y `boveda.json`**. Uno sin el otro no puede abrirse.
- **Qué protege y qué no:** protege tus conversaciones web en disco y en copias de seguridad mientras el archivo está cerrado. No cifra los ZIP originales que descargaste del navegador (guárdalos en un lugar cifrado o bórralos tras importar), las exportaciones `.md`/`.pdf` salen sin cifrar, y ningún cifrado en reposo protege frente a un programa malicioso ejecutándose en tu sesión mientras tienes el archivo desbloqueado.
- El contador de uso (`nisaba uso`) vive en tu índice local y nunca sale del equipo.
- Lo que excluyes con **No indexar** no se lee ni se copia; si lo conservas, ni la búsqueda ni el MCP lo muestran.

## Hoja de ruta

| Versión | Contenido |
|---|---|
| v0.1 | Claude Code + Codex, CLI, MCP, ventana, panel de salud |
| v0.1.1 | Exportar a `.md` y `.pdf` con aviso de secretos, volver a la sesión y la carpeta de origen, "No indexar", comandos también en inglés |
| **v0.1.2** | Archivo web cifrado (SQLCipher + Argon2id/AES-256-GCM con código de recuperación) e importador de exportaciones oficiales de Claude.ai (conversaciones, proyectos, memorias, *frames* y chats de diseño) con panel Detalles |
| v0.2 | Importador de exportaciones oficiales de ChatGPT sobre el archivo web cifrado |
| v0.3 | OpenCode |
| v0.4 | Pi |
| v0.5 | Gemini CLI |
| v0.6 | Antigravity (parcial) |
| v0.7 | Tema claro "Arcilla", barra lateral, atajo global, inglés en la interfaz |
| v1.0 | Todo lo anterior, estable |

## Compilar desde las fuentes

```bash
cargo test --workspace
npm --prefix crates/nisaba-gui/ui ci
npm --prefix crates/nisaba-gui/ui run tauri -- dev
```

## Licencia

[GPL-3.0-or-later](LICENSE) © 2026 D4vRAM369.
