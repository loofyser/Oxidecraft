//! Oxidecraft launcher: fetch and verify assets, then start the client.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use oxide_assets::fetch::{FetchOptions, fetch_version};
use oxide_assets::http::UreqClient;
use oxide_assets::store::Store;

#[derive(Parser)]
#[command(name = "oxide-launcher", version, about = "Oxidecraft launcher")]
struct Cli {
    /// Override the data directory.
    #[arg(long, global = true)]
    data_dir: Option<std::path::PathBuf>,
    /// Command to run.
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fetch and verify the game assets for a version.
    Fetch {
        /// Minecraft version, for example 1.8.9.
        #[arg(long, default_value = "1.8.9")]
        version: String,
        /// Re-hash everything already present.
        #[arg(long)]
        verify: bool,
        /// Resolve and report without downloading; the store directories are
        /// still created and the fetch lock is taken.
        #[arg(long, conflicts_with = "verify")]
        dry_run: bool,
    },
    /// Status ping a server.
    Ping {
        /// Host and port, for example 127.0.0.1:25565.
        address: String,
    },
}

/// The store root when `--data-dir` is absent: `<data dir>/oxidecraft`.
fn default_data_dir() -> anyhow::Result<PathBuf> {
    let base = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("no data directory is known on this platform"))?;
    Ok(base.join("oxidecraft"))
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let Cli { data_dir, command } = Cli::parse();
    match command {
        Command::Ping { address } => {
            let (host, port) = address
                .rsplit_once(':')
                .ok_or_else(|| anyhow::anyhow!("address must be host:port"))?;
            let status =
                oxide_proto_v47::status::ping_server(host, port.parse()?, Duration::from_secs(5))?;
            println!(
                "{} — protocol {} — {}/{} players",
                status.version.name,
                status.version.protocol,
                status.players.online,
                status.players.max
            );
            match status
                .description
                .and_then(|description| description.text())
            {
                Some(motd) => println!("MOTD: {motd}"),
                // Say so rather than staying silent: no plain-text MOTD is a
                // normal answer from a server that formats its description.
                None => println!("MOTD: (none)"),
            }
            Ok(())
        }
        Command::Fetch {
            version,
            verify,
            dry_run,
        } => {
            let store_root = match data_dir {
                Some(path) => path,
                None => default_data_dir()?,
            };
            let store = Store::open(store_root)?;
            let http = UreqClient::new();
            let options = FetchOptions {
                version,
                dry_run,
                verify,
            };

            let report = fetch_version(&store, &http, &options, |progress| {
                tracing::info!("{}: {}/{}", progress.name, progress.done, progress.total);
            })?;

            if dry_run {
                println!(
                    "dry run: {} file(s) already present, {} file(s) to download, {} bytes",
                    report.reused, report.planned, report.planned_bytes
                );
            } else {
                println!(
                    "fetch complete: {} downloaded, {} reused, {} bytes transferred",
                    report.downloaded, report.reused, report.bytes
                );
            }
            if let Some(verification) = &report.verification {
                println!(
                    "verify: {} objects, {} mismatched, {} missing, {} bytes on disk",
                    verification.objects,
                    verification.mismatched.len(),
                    verification.missing.len(),
                    verification.bytes
                );
                anyhow::ensure!(
                    verification.is_clean(),
                    "the verification pass found missing or mismatched objects"
                );
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    //! CLI parsing tests: the fetch flag combinations that must be refused.

    use super::{Cli, Command};
    use clap::Parser;

    #[test]
    fn a_dry_run_and_a_verify_together_are_refused() {
        let error = Cli::try_parse_from(["oxide-launcher", "fetch", "--dry-run", "--verify"])
            .err()
            .expect("the combination must not parse");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::ArgumentConflict,
            "{error}"
        );
    }

    #[test]
    fn a_dry_run_parses_alone_without_verify() {
        let cli = Cli::try_parse_from(["oxide-launcher", "fetch", "--dry-run"])
            .expect("a lone dry run parses");
        match cli.command {
            Command::Fetch {
                dry_run, verify, ..
            } => {
                assert!(dry_run, "the flag is set");
                assert!(!verify, "verify stays off");
            }
            _ => panic!("expected the fetch command"),
        }
    }
}
