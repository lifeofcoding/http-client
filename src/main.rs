mod parser;
mod runner;
mod server;

use clap::{Parser, Subcommand};
use parser::{ParseError, ParsedRequest, parse};
use runner::{Colors, execute, print_request};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "http-client",
    version,
    about = "Debugging tool that executes .http files against real or local endpoints"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Run {
        file: PathBuf,
        #[arg(short, long, value_name = "N")]
        request: Option<usize>,
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        verbose: bool,
        #[arg(long)]
        no_pretty: bool,
    },
    Serve {
        #[arg(long, default_value_t = 8080)]
        port: u16,
        #[arg(long, default_value_t = 200)]
        status: u16,
    },
    Demo,
}

fn main() -> ExitCode {
    install_crypto_provider();
    match Cli::parse().command {
        Command::Run {
            file,
            request,
            dry_run,
            verbose,
            no_pretty,
        } => cmd_run(file, request, dry_run, verbose, no_pretty),
        Command::Serve { port, status } => {
            server::run(server::ServeOptions { port, status });
            ExitCode::SUCCESS
        }
        Command::Demo => cmd_demo(),
    }
}

fn cmd_run(
    path: PathBuf,
    index: Option<usize>,
    dry_run: bool,
    verbose: bool,
    no_pretty: bool,
) -> ExitCode {
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    let requests = match parse(&content) {
        Ok(r) => r,
        Err(ParseError(msg)) => {
            eprintln!("error parsing {}: {msg}", path.display());
            return ExitCode::FAILURE;
        }
    };

    if requests.is_empty() {
        eprintln!("error: no requests found in {}", path.display());
        return ExitCode::FAILURE;
    }

    let selected: Vec<&ParsedRequest> = match index {
        Some(i) if (1..=requests.len()).contains(&i) => vec![&requests[i - 1]],
        Some(i) => {
            eprintln!(
                "error: --request {i} is out of range (file has {} request{})",
                requests.len(),
                if requests.len() == 1 { "" } else { "s" }
            );
            return ExitCode::FAILURE;
        }
        None => requests.iter().collect(),
    };

    let colors = Colors::auto();

    if dry_run {
        for (n, req) in selected.iter().enumerate() {
            if n > 0 {
                println!();
            }
            print_request(req);
        }
        return ExitCode::SUCCESS;
    }

    let client = match build_client() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error building HTTP client: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut failed = false;
    for (n, req) in selected.iter().enumerate() {
        if n > 0 {
            println!();
        }
        if let Err(e) = execute(&client, req, verbose, !no_pretty, &colors) {
            eprintln!("error: {e}");
            failed = true;
        }
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_demo() -> ExitCode {
    let port = server::spawn(200);
    println!("echo server on http://127.0.0.1:{port}\n");

    let base = format!("http://127.0.0.1:{port}");
    let requests = [ParsedRequest {
            method: "GET".into(),
            url: format!("{base}/hello?name=world"),
            headers: vec![("Accept".into(), "application/json".into())],
            body: None,
        },
        ParsedRequest {
            method: "POST".into(),
            url: format!("{base}/api/cleanupVoices"),
            headers: vec![
                ("Content-Type".into(), "application/json".into()),
                ("Authorization".into(), "Bearer demo-token".into()),
            ],
            body: Some("{\n  \"limit\": 300\n}".into()),
        }];

    let client = match build_client() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error building HTTP client: {e}");
            return ExitCode::FAILURE;
        }
    };

    let colors = Colors::auto();
    let mut failed = false;
    for (n, req) in requests.iter().enumerate() {
        if n > 0 {
            println!();
        }
        if let Err(e) = execute(&client, req, false, true, &colors) {
            eprintln!("error: {e}");
            failed = true;
        }
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn build_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("http-client/0.1")
        .build()
        .map_err(|e| e.to_string())
}

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
