use clap::{Parser, Subcommand};
use iroh::NodeId;

#[derive(Parser, Debug)]
pub struct Opts {
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the iroh tunnel server
    Server { ports: Vec<u16> },
    /// Start the iroh tunnel client
    Client {
        id: NodeId,
        #[clap(value_parser = parse_mapping)]
        mapping: (u16, u16),
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
