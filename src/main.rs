mod parser;
mod runner;
mod server;
mod vars;

use clap::{Parser, Subcommand};
use parser::{ParseError, ParsedRequest, parse};
use runner::{Colors, execute, print_request};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use vars::Secrets;

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
        #[arg(long, value_name = "FILE")]
        env_file: Vec<PathBuf>,
        #[arg(long)]
        no_redact: bool,
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
            env_file,
            no_redact,
        } => cmd_run(
            file, request, dry_run, verbose, no_pretty, env_file, no_redact,
        ),
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
    env_files: Vec<PathBuf>,
    no_redact: bool,
) -> ExitCode {
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    let mut requests = match parse(&content) {
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

    let selected_idx: Vec<usize> = match index {
        Some(i) if (1..=requests.len()).contains(&i) => vec![i - 1],
        Some(i) => {
            eprintln!(
                "error: --request {i} is out of range (file has {} request{})",
                requests.len(),
                if requests.len() == 1 { "" } else { "s" }
            );
            return ExitCode::FAILURE;
        }
        None => (0..requests.len()).collect(),
    };

    let mut secrets = match Secrets::load(&env_files) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut malformed: Vec<usize> = Vec::new();
    let mut unresolved: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &i in &selected_idx {
        let req = &mut requests[i];
        subst_field(
            &mut secrets,
            &mut req.url,
            i,
            &mut malformed,
            &mut unresolved,
        );
        for (name, value) in &mut req.headers {
            subst_field(&mut secrets, name, i, &mut malformed, &mut unresolved);
            subst_field(&mut secrets, value, i, &mut malformed, &mut unresolved);
        }
        if let Some(body) = &mut req.body {
            subst_field(&mut secrets, body, i, &mut malformed, &mut unresolved);
        }
    }

    if !malformed.is_empty() || !unresolved.is_empty() {
        eprintln!("error: variable substitution failed in {}:", path.display());
        for (name, reqs) in &unresolved {
            eprintln!("  {name} ({})", fmt_request_numbers(reqs));
        }
        for &i in &malformed {
            eprintln!("  unterminated {{{{ placeholder (request {})", i + 1);
        }
        return ExitCode::FAILURE;
    }

    let selected: Vec<&ParsedRequest> = selected_idx.iter().map(|&i| &requests[i]).collect();
    let masked: &[String] = if no_redact {
        &[]
    } else {
        secrets.masked_values()
    };

    let colors = Colors::auto();

    if dry_run {
        for (n, req) in selected.iter().enumerate() {
            if n > 0 {
                println!();
            }
            print_request(req, masked);
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
        if let Err(e) = execute(&client, req, verbose, !no_pretty, &colors, masked) {
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
    let mut secrets = Secrets::from_maps(
        HashMap::from([(
            String::from("DEMO_TOKEN"),
            std::env::var("DEMO_TOKEN").unwrap_or_else(|_| String::from("demo-token")),
        )]),
        HashMap::new(),
    );

    let token_header = secrets
        .substitute("Bearer {{DEMO_TOKEN}}")
        .unwrap_or_else(|_| String::from("Bearer {{DEMO_TOKEN}}"));

    let requests = [
        ParsedRequest {
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
        },
        ParsedRequest {
            method: "GET".into(),
            url: format!("{base}/auth"),
            headers: vec![("Authorization".into(), token_header)],
            body: None,
        },
    ];

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
        if let Err(e) = execute(&client, req, false, true, &colors, secrets.masked_values()) {
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

fn subst_field(
    secrets: &mut Secrets,
    field: &mut String,
    index: usize,
    malformed: &mut Vec<usize>,
    unresolved: &mut BTreeMap<String, Vec<usize>>,
) {
    match secrets.substitute(field.as_str()) {
        Ok(resolved) => *field = resolved,
        Err(e) => {
            if e.malformed && malformed.last() != Some(&index) {
                malformed.push(index);
            }
            for name in e.unresolved {
                let reqs = unresolved.entry(name).or_default();
                if reqs.last() != Some(&index) {
                    reqs.push(index);
                }
            }
        }
    }
}

fn fmt_request_numbers(reqs: &[usize]) -> String {
    let nums: Vec<String> = reqs.iter().map(|i| (i + 1).to_string()).collect();
    if nums.len() == 1 {
        format!("request {}", nums[0])
    } else {
        format!("requests {}", nums.join(", "))
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
