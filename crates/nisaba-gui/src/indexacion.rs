use nisaba_core::{
    db,
    indexador::{indexar, Progreso, ResultadoIndexado, Rutas},
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::Emitter;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct EstadoIndexacion {
    pub estado: &'static str,
    pub vistos: usize,
    pub total: usize,
    pub mensaje: Option<String>,
}

impl Default for EstadoIndexacion {
    fn default() -> Self {
        Self {
            estado: "indexando",
            vistos: 0,
            total: 0,
            mensaje: None,
        }
    }
}

#[derive(Default)]
struct Interno {
    iniciado: AtomicBool,
    indexando: AtomicBool,
    estado: Mutex<EstadoIndexacion>,
}

#[derive(Clone, Default)]
pub struct Indexacion(Arc<Interno>);

impl Indexacion {
    pub fn estado(&self) -> EstadoIndexacion {
        self.0
            .estado
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn iniciar(&self, app: tauri::AppHandle) {
        if self.0.iniciado.swap(true, Ordering::AcqRel) {
            return;
        }
        let control = self.clone();
        std::thread::spawn(move || control.vigilar(app, Rutas::desde_entorno()));
    }

    fn emitir(&self, app: &tauri::AppHandle, estado: EstadoIndexacion) {
        *self.0.estado.lock().unwrap_or_else(|e| e.into_inner()) = estado.clone();
        if let Err(e) = app.emit("indexacion", &estado) {
            eprintln!("No se pudo emitir el estado de indexación: {e}");
        }
    }

    fn pasada(&self, app: &tauri::AppHandle, rutas: &Rutas, aviso: Option<&str>) {
        if self.0.indexando.swap(true, Ordering::AcqRel) {
            return;
        }
        struct Liberar<'a>(&'a AtomicBool);
        impl Drop for Liberar<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _liberar = Liberar(&self.0.indexando);
        let mut progreso = Progreso {
            vistos: 0,
            total: 0,
        };
        self.emitir(app, EstadoIndexacion::default());
        let mut ultimo = Instant::now();
        let resultado =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match db::abrir(&rutas.home.join("tablilla.db")) {
                    Ok(mut conn) => indexar(&mut conn, rutas, &mut |p| {
                        progreso = p;
                        if ultimo.elapsed() >= Duration::from_millis(500) {
                            self.emitir(
                                app,
                                EstadoIndexacion {
                                    estado: "indexando",
                                    vistos: p.vistos,
                                    total: p.total,
                                    mensaje: None,
                                },
                            );
                            ultimo = Instant::now();
                        }
                    }),
                    Err(e) => ResultadoIndexado::Fallo(e.to_string()),
                }
            }))
            .unwrap_or_else(|_| {
                ResultadoIndexado::Fallo(
                    "Fallo interno al indexar; se conserva el índice anterior".into(),
                )
            });
        let mut estado = finalizar(resultado, progreso);
        if let Some(aviso) = aviso {
            estado.estado = "fallo";
            estado.mensaje = Some(match estado.mensaje {
                Some(m) => format!("{m}. {aviso}"),
                None => aviso.to_owned(),
            });
        }
        self.emitir(app, estado);
    }

    fn vigilar(&self, app: tauri::AppHandle, rutas: Rutas) {
        // Un único aviso pendiente agrupa ráfagas sin acumular eventos mientras se indexa.
        let (tx, rx) = mpsc::sync_channel(1);
        let mut watcher = notify::recommended_watcher(
            move |evento: notify::Result<notify::Event>| match evento {
                Ok(e) if evento_relevante(&e) => {
                    let _ = tx.try_send(Ok(()));
                }
                Err(e) => {
                    let _ = tx.try_send(Err(format!("Vigilancia de fuentes: {e}")));
                }
                _ => {}
            },
        )
        .ok();
        let mut vigiladas = HashSet::new();
        let mut aviso = registrar_fuentes(watcher.as_mut(), &rutas, &mut vigiladas);
        self.pasada(&app, &rutas, aviso.as_deref());
        let mut reintentar_ocupado = self.estado().estado == "ocupado";
        loop {
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(evento) => {
                    if let Err(e) = evento {
                        aviso = Some(e);
                    }
                    // Espera dos segundos desde el último cambio; no pierde cambios durante una pasada.
                    while let Ok(evento) = rx.recv_timeout(Duration::from_secs(2)) {
                        if let Err(e) = evento {
                            aviso = Some(e);
                        }
                    }
                    if let Some(e) = registrar_fuentes(watcher.as_mut(), &rutas, &mut vigiladas) {
                        aviso = Some(e);
                    }
                    self.pasada(&app, &rutas, aviso.as_deref());
                    reintentar_ocupado = self.estado().estado == "ocupado";
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let antes = vigiladas.clone();
                    let nuevo_aviso = registrar_fuentes(watcher.as_mut(), &rutas, &mut vigiladas);
                    if antes != vigiladas || reintentar_ocupado || aviso.is_some() {
                        aviso = nuevo_aviso;
                        self.pasada(&app, &rutas, aviso.as_deref());
                        reintentar_ocupado = self.estado().estado == "ocupado";
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Sin watcher funcional, el sondeo conserva la bóveda actualizable y avisa.
                    std::thread::sleep(Duration::from_secs(2));
                    aviso = registrar_fuentes(watcher.as_mut(), &rutas, &mut vigiladas);
                    self.pasada(&app, &rutas, aviso.as_deref());
                }
            }
        }
    }
}

fn registrar_fuentes(
    watcher: Option<&mut RecommendedWatcher>,
    rutas: &Rutas,
    vigiladas: &mut HashSet<PathBuf>,
) -> Option<String> {
    let Some(watcher) = watcher else {
        return Some(
            "No se pudo iniciar la vigilancia de archivos; se reintenta por sondeo".into(),
        );
    };
    let mut errores = Vec::new();
    for ruta in [&rutas.claude, &rutas.codex] {
        if !ruta.is_dir() {
            if vigiladas.remove(ruta) {
                let _ = watcher.unwatch(ruta);
            }
            errores.push(format!("Fuente no disponible: {}", ruta.display()));
        } else if !vigiladas.contains(ruta) {
            match watcher.watch(ruta, RecursiveMode::Recursive) {
                Ok(()) => {
                    vigiladas.insert(ruta.clone());
                }
                Err(e) => errores.push(format!("No se puede vigilar {}: {e}", ruta.display())),
            }
        }
    }
    (!errores.is_empty()).then(|| errores.join(". "))
}

fn finalizar(resultado: ResultadoIndexado, p: Progreso) -> EstadoIndexacion {
    let (estado, mensaje) = match resultado {
        ResultadoIndexado::Hecho { errores: 0, .. } => ("hecho", None),
        ResultadoIndexado::Hecho { errores, .. } => (
            "fallo",
            Some(format!("{errores} archivos con error. Consulta Salud")),
        ),
        ResultadoIndexado::Ocupado => (
            "ocupado",
            Some("Otro proceso está indexando; puedes buscar en lo que ya hay".into()),
        ),
        ResultadoIndexado::Fallo(e) => (
            "fallo",
            Some(format!(
                "No se pudo indexar: {e}. Puedes buscar en lo que ya hay"
            )),
        ),
    };
    EstadoIndexacion {
        estado,
        vistos: p.vistos,
        total: p.total,
        mensaje,
    }
}

fn evento_relevante(evento: &notify::Event) -> bool {
    !matches!(evento.kind, notify::EventKind::Access(_))
        && (evento.paths.is_empty()
            || evento.paths.iter().any(|p| {
                p.extension().is_none()
                    || p.is_dir()
                    || p.to_str().is_some_and(|p| {
                        p.ends_with(".jsonl")
                            || p.ends_with(".jsonl.zst")
                            || p.ends_with(".meta.json")
                    })
            }))
}

#[tauri::command]
pub fn iniciar(app: tauri::AppHandle, control: tauri::State<'_, Indexacion>) -> EstadoIndexacion {
    control.iniciar(app);
    control.estado()
}

#[tauri::command]
pub fn estado_indexacion(control: tauri::State<'_, Indexacion>) -> EstadoIndexacion {
    control.estado()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ocupado_y_fallo_no_se_presentan_como_al_dia() {
        let p = Progreso {
            vistos: 4,
            total: 10,
        };
        let ocupado = finalizar(ResultadoIndexado::Ocupado, p);
        assert_eq!(ocupado.estado, "ocupado");
        assert!(ocupado.mensaje.is_some());
        let fallo = finalizar(ResultadoIndexado::Fallo("disco inaccesible".into()), p);
        assert_eq!(fallo.estado, "fallo");
        assert!(fallo.mensaje.unwrap().contains("disco inaccesible"));
    }
    #[test]
    fn final_conserva_progreso_y_avisa_archivos_con_error() {
        let p = Progreso {
            vistos: 10,
            total: 10,
        };
        let hecho = finalizar(
            ResultadoIndexado::Hecho {
                nuevos: 1,
                cambiados: 0,
                errores: 0,
            },
            p,
        );
        assert_eq!(hecho.estado, "hecho");
        assert_eq!(hecho.vistos, 10);
        let error = finalizar(
            ResultadoIndexado::Hecho {
                nuevos: 0,
                cambiados: 0,
                errores: 2,
            },
            p,
        );
        assert_eq!(error.estado, "fallo");
        assert!(error.mensaje.unwrap().contains('2'));
    }
    #[test]
    fn leer_fuentes_no_dispara_otra_indexacion() {
        let e = notify::Event::new(notify::EventKind::Access(notify::event::AccessKind::Read));
        assert!(!evento_relevante(&e));
        let e = notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any,
        )));
        assert!(evento_relevante(&e));
    }
}
