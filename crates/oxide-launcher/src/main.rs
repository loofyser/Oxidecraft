//! Oxidecraft launcher: fetch and verify assets, then start the client.

use std::time::Duration;

use clap::{Parser, Subcommand};

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
        /// Resolve and report without downloading.
        #[arg(long)]
        dry_run: bool,
    },
    /// Status ping a server.
    Ping {
        /// Host and port, for example 127.0.0.1:25565.
        address: String,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
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
            let _ = (version, verify, dry_run);
            anyhow::bail!("the fetch command is not available yet")
        }
    }
}
