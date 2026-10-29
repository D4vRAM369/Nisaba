mod comun;
use comun::ctx;
use serde_json::{json, Value};
use std::io::Write;
use std::process::Stdio;

/// Escribe todas las peticiones, cierra stdin y devuelve (líneas de stdout, código de salida).
fn sesion(peticiones: &[Value]) -> (Vec<String>, Option<i32>) {
    let c = ctx();
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
    let lineas = String::from_utf8(o.stdout).unwrap().lines().map(String::from).collect();
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
        req(1, "initialize", json!({"protocolVersion":"2024-11-05","capabilities":{}})),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        req(2, "tools/list", json!({})),
    ]);
    assert_eq!(cod, Some(0));
    assert_eq!(l.len(), 2, "{l:?}");
    let a = parse(&l[0]);
    assert_eq!(a["result"]["serverInfo"]["name"], "nisaba");
    assert_eq!(a["result"]["protocolVersion"], "2024-11-05");
    let b = parse(&l[1]);
    let nombres: Vec<&str> = b["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(nombres, ["buscar_conversaciones", "leer_conversacion"]);
}

#[test]
fn llamada_buscar() {
    let (l, _) = sesion(&[llamada(1, "buscar_conversaciones", json!({"consulta":"subtitulos"}))]);
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
    let mut hijo = c.comando(&["mcp"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    {
        let mut si = hijo.stdin.take().unwrap();
        writeln!(si, "esto no es json").unwrap();
        writeln!(si, "{}", json!({"jsonrpc":"2.0","id":7})).unwrap();
    }
    let o = hijo.wait_with_output().unwrap();
    let l: Vec<Value> = String::from_utf8(o.stdout).unwrap().lines().map(parse).collect();
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
