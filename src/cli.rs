use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
pub struct Opts {
    #[clap(subcommand)]
    pub command: Command,

    /// Use an ephemeral Node ID (randomly generated key pair)
    #[clap(short, long)]
    pub ephemeral: bool,

    /// Path to the private key file
    #[clap(short, long)]
    pub private_key: Option<PathBuf>,

    /// Force the regeneration of the private key
    #[clap(short, long)]
    pub regenerate: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the iroh tunnel server
    Server {},
    /// Start the iroh tunnel client
    Client {
        /// Identifier of the host to connect to (Node ID or name)
        to: String,

        /// Port mapping in the format "local:remote"
        #[clap(value_parser = parse_mapping)]
        mapping: (u16, u16),

        /// Protocol to use for the connection
        #[clap(short, long, default_value = "tcp")]
        protocol: Protocol,
    },

    /// Display our Node ID
    Id {
        /// Short form of the Node ID
        #[clap(short, long)]
        short: bool,
    },

    /// Manage known hosts
    Hosts {
        #[clap(subcommand)]
        command: HostCommand,
    },
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl std::str::FromStr for Protocol {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tcp" => Ok(Protocol::Tcp),
            "udp" => Ok(Protocol::Udp),
            _ => Err("Invalid protocol. Use 'tcp' or 'udp'.".to_string()),
        }
    }
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
    Remove {
        /// Name or Node ID of the host to remove
        identifier: String,
    },
    /// List all known hosts
    List {
        /// Show the full Node ID
        #[clap(short, long)]
        full: bool,
    },
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
