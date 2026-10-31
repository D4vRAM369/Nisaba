mod comun;
use comun::ctx;
use nisaba_core::archivo_web;
use nisaba_core::boveda::{self, BOVEDA, TABLILLA_WEB};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::process::Stdio;

/// Escribe todas las peticiones, cierra stdin y devuelve (líneas de stdout, código de salida).
fn sesion(peticiones: &[Value]) -> (Vec<String>, Option<i32>) {
    sesion_en(&ctx(), peticiones)
}

fn sesion_en(c: &comun::Ctx, peticiones: &[Value]) -> (Vec<String>, Option<i32>) {
    let mut hijo = c
        .comando(&["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut si = hijo.stdin.take().unwrap();
        for p in peticiones {
            writeln!(si, "{p}").unwrap();
        }
    }
    let o = hijo.wait_with_output().unwrap();
    let lineas = String::from_utf8(o.stdout)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    (lineas, o.status.code())
}

fn req(id: i64, metodo: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":metodo,"params":params})
}

fn llamada(id: i64, nombre: &str, args: Value) -> Value {
    req(id, "tools/call", json!({"name": nombre, "arguments": args}))
}

fn parse(l: &str) -> Value {
    serde_json::from_str(l).unwrap()
}

#[test]
fn initialize_y_lista() {
    let (l, cod) = sesion(&[
        req(
            1,
            "initialize",
            json!({"protocolVersion":"2024-11-05","capabilities":{}}),
        ),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        req(2, "tools/list", json!({})),
    ]);
    assert_eq!(cod, Some(0));
    assert_eq!(l.len(), 2, "{l:?}");
    let a = parse(&l[0]);
    assert_eq!(a["result"]["serverInfo"]["name"], "nisaba");
    assert_eq!(a["result"]["protocolVersion"], "2024-11-05");
    let b = parse(&l[1]);
    let nombres: Vec<&str> = b["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(nombres, ["buscar_conversaciones", "leer_conversacion"]);
    let descripcion = b["result"]["tools"][0]["description"].as_str().unwrap();
    assert!(
        descripcion.contains("del usuario") && !descripcion.contains("Brian"),
        "{descripcion}"
    );
}

#[test]
fn llamada_buscar() {
    let (l, _) = sesion(&[llamada(
        1,
        "buscar_conversaciones",
        json!({"consulta":"subtitulos"}),
    )]);
    let r = parse(&l[0]);
    let t = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t.contains("cx-cx01"), "{t}");
    assert!(t.contains("alrededor_de"), "{t}");
    assert!(!t.contains('\x1b'));
}

#[test]
fn llamada_leer() {
    let (l, _) = sesion(&[llamada(1, "leer_conversacion", json!({"id":"cx-cx01"}))]);
    let r = parse(&l[0]);
    let t = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t.contains("### 👤 Tú"), "{t}");
}

#[test]
fn errores_de_herramienta() {
    let (l, _) = sesion(&[
        llamada(1, "leer_conversacion", json!({"id":"no-existe"})),
        llamada(2, "inventada", json!({})),
        llamada(3, "buscar_conversaciones", json!({})),
    ]);
    assert_eq!(l.len(), 3);
    for x in &l {
        let r = parse(x);
        assert_eq!(r["result"]["isError"], true, "{x}");
    }
}

#[test]
fn metodo_desconocido() {
    let (l, _) = sesion(&[req(1, "no/existe", json!({}))]);
    assert_eq!(parse(&l[0])["error"]["code"], -32601);
}

#[test]
fn json_invalido_y_estructura_invalida() {
    let c = ctx();
    let mut hijo = c
        .comando(&["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut si = hijo.stdin.take().unwrap();
        writeln!(si, "esto no es json").unwrap();
        writeln!(si, "{}", json!({"jsonrpc":"2.0","id":7})).unwrap();
    }
    let o = hijo.wait_with_output().unwrap();
    let l: Vec<Value> = String::from_utf8(o.stdout)
        .unwrap()
        .lines()
        .map(parse)
        .collect();
    assert_eq!(l[0]["error"]["code"], -32700);
    assert_eq!(l[1]["error"]["code"], -32600);
    assert_eq!(l[1]["id"], 7);
}

#[test]
fn stdout_solo_json() {
    let (l, cod) = sesion(&[
        req(1, "initialize", json!({})),
        req(2, "tools/list", json!({})),
        llamada(3, "buscar_conversaciones", json!({"consulta":"subtitulos"})),
        llamada(4, "leer_conversacion", json!({"id":"cx-cx01"})),
        req(5, "no/existe", json!({})),
    ]);
    assert_eq!(cod, Some(0));
    assert_eq!(l.len(), 5);
    for x in &l {
        assert_eq!(parse(x)["jsonrpc"], "2.0", "{x}");
    }
}

#[test]
fn bytes_no_utf8_no_matan_el_servidor() {
    let c = ctx();
    let mut hijo = c
        .comando(&["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut si = hijo.stdin.take().unwrap();
        si.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"\xff\xfe\"}\n")
            .unwrap();
        si.write_all(b"\xc3\x28\xff\n").unwrap();
        writeln!(si, "{}", req(2, "tools/list", json!({}))).unwrap();
    }
    let o = hijo.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    let l: Vec<Value> = String::from_utf8(o.stdout)
        .unwrap()
        .lines()
        .map(parse)
        .collect();
    assert_eq!(l.len(), 3);
    assert_eq!(l[0]["error"]["code"], -32601);
    assert_eq!(l[1]["error"]["code"], -32700);
    assert_eq!(l[2]["id"], 2);
    assert!(l[2]["result"]["tools"].is_array());
}

fn texto(l: &str) -> String {
    parse(l)["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn mcp_buscar_incluye_cwd() {
    let (l, _) = sesion(&[
        llamada(1, "buscar_conversaciones", json!({"consulta":"subtitulos"})),
        llamada(2, "leer_conversacion", json!({"id":"cx-cx01"})),
    ]);
    for x in &l {
        let t = texto(x);
        assert!(t.contains("/home/u/Proyectos/Mirlo"), "{t}");
        assert!(t.contains("cx01"), "{t}");
    }
    assert!(
        texto(&l[0]).contains("id_original: cx01"),
        "{}",
        texto(&l[0])
    );
}

#[test]
fn mcp_leer_excluida() {
    let c = ctx();
    c.nisaba(&["indexar"]);
    let o = c.nisaba(&["excluir", "/home/u/Proyectos/Demo"]);
    assert_eq!(o.status.code(), Some(0));
    let (l, _) = sesion_en(
        &c,
        &[llamada(1, "leer_conversacion", json!({"id":"cc-a1b2c3d4"}))],
    );
    let r = parse(&l[0]);
    assert_eq!(r["result"]["isError"], true, "{r}");
    let t = texto(&l[0]);
    assert!(t.contains("Conversación excluida por el usuario"), "{t}");
    assert!(!t.contains("PillMate"), "{t}");
}

/// Un archivo web real con el marcador y `boveda.json` estropeada o ausente: si el MCP lo abriese,
/// fallaría; si lo leyese de algún modo, saldría el marcador.
#[test]
fn mcp_no_abre_archivo_web() {
    const MARCADOR: &str = "ZorroVioletaMarcador";
    let c = ctx();
    let casa = c.dir.path().join("home");
    fs::create_dir_all(&casa).unwrap();
    let llave = boveda::preparar("correcta caballo pila")
        .unwrap()
        .guardar(&casa)
        .unwrap();
    let web = archivo_web::abrir(&casa, &llave).unwrap();
    web.execute_batch(&format!(
        "INSERT INTO conversaciones(id, fuente, id_original) VALUES (1, 'claude-web', 'web00001');
         INSERT INTO mensajes(id, conversacion_id, orden, rol, oculto) VALUES (1, 1, 0, 'usuario', 0);
         INSERT INTO partes(id, mensaje_id, orden, tipo, contenido) VALUES (1, 1, 0, 'texto', '{MARCADOR}');"
    ))
    .unwrap();
    drop(web);

    for vuelta in ["corrupta", "ausente", "tablilla corrupta"] {
        match vuelta {
            "corrupta" => fs::write(casa.join(BOVEDA), "no soy una bóveda").unwrap(),
            "ausente" => fs::remove_file(casa.join(BOVEDA)).unwrap(),
            _ => fs::write(casa.join(TABLILLA_WEB), "no soy una tablilla").unwrap(),
        }
        let en_disco = fs::read(casa.join(TABLILLA_WEB)).unwrap();
        let (l, cod) = sesion_en(
            &c,
            &[
                llamada(1, "buscar_conversaciones", json!({"consulta": MARCADOR})),
                llamada(2, "leer_conversacion", json!({"id": "cw-web00001"})),
            ],
        );
        assert_eq!(cod, Some(0), "{vuelta}");
        assert_eq!(l.len(), 2, "{vuelta}: {l:?}");
        assert_eq!(texto(&l[0]), "Sin resultados.", "{vuelta}");
        assert!(texto(&l[1]).contains("No encuentro"), "{vuelta}");
        assert!(l.iter().all(|x| !x.contains(MARCADOR)), "{vuelta}: {l:?}");
        assert_eq!(fs::read(casa.join(TABLILLA_WEB)).unwrap(), en_disco);
    }
}

#[test]
fn mcp_no_menciona_el_archivo_web_en_su_codigo() {
    let codigo = include_str!("../src/mcp.rs");
    for prohibido in ["archivo_web", "boveda", "crate::web"] {
        assert!(!codigo.contains(prohibido), "mcp.rs usa {prohibido}");
    }
}

#[test]
fn mcp_rechaza_las_fuentes_web() {
    for fuente in ["claude-web", "chatgpt"] {
        let (l, _) = sesion(&[llamada(
            1,
            "buscar_conversaciones",
            json!({"consulta": "x", "fuente": fuente}),
        )]);
        let r = parse(&l[0]);
        assert_eq!(r["result"]["isError"], true, "{r}");
        assert!(texto(&l[0]).contains("claude-code o codex"), "{r}");
    }
}
