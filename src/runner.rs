use crate::parser::ParsedRequest;
use std::io::IsTerminal;
use std::time::{Duration, Instant};

pub struct Colors {
    enabled: bool,
}

impl Colors {
    pub fn auto() -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some();
        let tty = std::io::stdout().is_terminal();
        Colors {
            enabled: !no_color && tty,
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("{code}{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn dim(&self, text: &str) -> String {
        self.paint("\x1b[2m", text)
    }

    fn status(&self, status: u16) -> String {
        let code = match status {
            200..=299 => "\x1b[32m",
            300..=499 => "\x1b[33m",
            _ => "\x1b[31m",
        };
        self.paint(code, &status.to_string())
    }
}

pub fn print_request(req: &ParsedRequest) {
    println!("{} {}", req.method, req.url);
    for (name, value) in &req.headers {
        println!("  {name}: {value}");
    }
    if let Some(body) = &req.body {
        println!();
        for line in body.lines() {
            println!("  {line}");
        }
    }
}

pub fn execute(
    client: &reqwest::blocking::Client,
    req: &ParsedRequest,
    verbose: bool,
    pretty: bool,
    colors: &Colors,
) -> Result<(), String> {
    let method = reqwest::Method::from_bytes(req.method.as_bytes())
        .map_err(|e| format!("invalid method `{}`: {e}", req.method))?;

    let mut builder = client.request(method, req.url.as_str());
    for (name, value) in &req.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    if let Some(body) = &req.body {
        builder = builder.body(body.clone());
    }

    println!("{} {} {}", colors.dim("→"), req.method, req.url);

    let start = Instant::now();
    let response = builder
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("{e}"))?;
    let elapsed = start.elapsed();

    let status = response.status();
    let reason = status.canonical_reason().unwrap_or("");
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    println!(
        "{} {} · {}",
        colors.dim("←"),
        status_label(status, reason, colors),
        colors.dim(&fmt_duration(elapsed)),
    );

    if verbose {
        let mut names: Vec<&str> = response.headers().keys().map(|k| k.as_str()).collect();
        names.sort_unstable();
        for name in names {
            for value in response.headers().get_all(name) {
                let value = value.to_str().unwrap_or("<binary>");
                println!("  {}: {}", colors.dim(name), value);
            }
        }
    }

    let bytes = response.bytes().map_err(|e| format!("reading body: {e}"))?;
    if !bytes.is_empty() {
        println!("{}", render_body(&bytes, &content_type, pretty));
    }

    Ok(())
}

fn status_label(status: reqwest::StatusCode, reason: &str, colors: &Colors) -> String {
    let code = colors.status(status.as_u16());
    if reason.is_empty() {
        code
    } else {
        format!("{code} {reason}")
    }
}

fn render_body(bytes: &[u8], content_type: &str, pretty: bool) -> String {
    if pretty && content_type.contains("json")
        && let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes)
            && let Ok(formatted) = serde_json::to_string_pretty(&value) {
                return formatted;
            }
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn fmt_duration(elapsed: Duration) -> String {
    let micros = elapsed.as_micros();
    if micros < 1_000 {
        format!("{micros}µs")
    } else if micros < 1_000_000 {
        format!("{:.1}ms", micros as f64 / 1_000.0)
    } else {
        format!("{:.2}s", micros as f64 / 1_000_000.0)
    }
}
