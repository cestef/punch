use crate::core::Protocol;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Opts {
    #[clap(subcommand)]
    pub command: Command,

    /// Use an ephemeral Node ID (randomly generated key pair)
    #[clap(short, long, global = true)]
    pub ephemeral: bool,

    /// Path to the private key file
    #[clap(short, long, global = true)]
    pub private_key: Option<PathBuf>,

    /// Force the regeneration of the private key
    #[clap(short, long, global = true)]
    pub regenerate: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the iroh tunnel server
    #[command(visible_alias = "s")]
    Server {},

    /// Start the iroh tunnel client
    #[command(visible_alias = "c")]
    Client {
        /// Identifier of the host to connect to (Node ID or name)
        to: String,

        /// Port mapping in the format "local:remote"
        #[clap(value_parser = parse_mapping)]
        mapping: (u16, u16),

        /// Protocol to use for the connection
        #[clap(short = 'P', long, default_value = "tcp")]
        protocol: Protocol,
    },

    /// Display our Node ID
    Id {
        /// Short form of the Node ID
        #[clap(short, long)]
        short: bool,
    },

    /// Manage known hosts (client)
    #[command(visible_alias = "h")]
    Hosts {
        #[clap(subcommand)]
        command: HostCommand,
    },

    /// Manage authorization (server)
    #[command(visible_alias = "a")]
    Auth {
        #[clap(subcommand)]
        command: AuthCommand,
    },

    /// Show configuration information
    Config {
        /// Show the configuration directory path
        #[clap(short, long)]
        show_path: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum HostCommand {
    /// Add a new host
    Add {
        /// Name of the host
        name: String,
        /// Node ID of the host
        id: String,
    },

    /// Remove a host by name or ID
    #[command(visible_alias = "rm")]
    Remove {
        /// Name or Node ID of the host to remove
        identifier: String,
    },

    /// List all known hosts
    #[command(visible_alias = "ls")]
    List {
        /// Show the full Node ID
        #[clap(short, long)]
        full: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// List authorized keys
    #[command(visible_alias = "ls")]
    List,

    /// Add an authorized key
    Add {
        /// Public key to authorize
        key: String,
    },

    /// Remove an authorized key
    #[command(visible_alias = "rm")]
    Remove {
        /// Public key to remove
        key: String,
    },

    /// Show your public key
    #[command(name = "my-key")]
    MyKey,
}

fn parse_mapping(s: &str) -> Result<(u16, u16), String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err("Mapping must be in the format 'local_port:remote_port'".to_string());
    }
    let local_port = parts[0]
        .parse::<u16>()
        .map_err(|_| "Invalid local port".to_string())?;
    let remote_port = parts[1]
        .parse::<u16>()
        .map_err(|_| "Invalid remote port".to_string())?;
    Ok((local_port, remote_port))
}
