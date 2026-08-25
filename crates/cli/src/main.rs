//! `surge` CLI — thin shim over the loopback API (code-map: cli).
//! `auth`, `compile`, `dispatch`, `abort` land with their server endpoints.

use clap::{Parser, Subcommand};

const API: &str = "http://127.0.0.1:7420";

#[derive(Parser)]
#[command(name = "surge", version, about = "Surge — local pipeline orchestrator")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show whether a Surge instance is running and its versions.
    Status,
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Command::Status => {
            match ureq::get(&format!("{API}/healthz")).call() {
                Ok(resp) => println!("surge running at {API}: {}", resp.into_string()?),
                Err(_) => println!("no surge instance reachable at {API}"),
            }
        }
    }
    Ok(())
}
