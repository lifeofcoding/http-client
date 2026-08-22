use serde_json::json;
use std::sync::mpsc;
use std::thread;
use tiny_http::{Header, Response, Server};

pub struct ServeOptions {
    pub port: u16,
    pub status: u16,
}

pub fn run(opts: ServeOptions) {
    let addr = format!("127.0.0.1:{}", opts.port);
    let server = match Server::http(addr.as_str()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    println!("echo server listening on http://{addr} (Ctrl-C to stop)");
    serve_loop(server, opts.status);
}

pub fn spawn(status: u16) -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe listener");
    let port = probe.local_addr().expect("local addr").port();
    drop(probe);

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || match Server::http(("127.0.0.1", port)) {
        Ok(server) => {
            tx.send(port).ok();
            serve_loop(server, status);
        }
        Err(e) => {
            eprintln!("error: cannot bind demo server: {e}");
            std::process::exit(1);
        }
    });
    rx.recv().expect("demo server failed to start")
}

fn serve_loop(server: Server, status: u16) {
    for request in server.incoming_requests() {
        handle(request, status);
    }
}

fn handle(mut request: tiny_http::Request, status: u16) {
    let method = request.method().as_str().to_string();
    let path = request.url().to_string();

    let headers: Vec<_> = request
        .headers()
        .iter()
        .map(|h| json!({ "name": h.field.to_string(), "value": h.value.to_string() }))
        .collect();

    let mut buf = Vec::new();
    let _ = request.as_reader().read_to_end(&mut buf);
    let body = String::from_utf8_lossy(&buf).into_owned();

    println!("{method} {path} ({} byte body)", body.len());

    let payload = json!({
        "method": method,
        "path": path,
        "headers": headers,
        "bodyLength": buf.len(),
        "body": body,
    });
    let data = serde_json::to_string_pretty(&payload).expect("serialize echo payload");

    let content_type =
        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).expect("valid header");
    let response = Response::from_string(data)
        .with_header(content_type)
        .with_status_code(status);
    let _ = request.respond(response);
}
