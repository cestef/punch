use anyhow::Result;
use clap::Parser;

use punch::{
    cli::{Command, Opts},
    core::{build_endpoint, client::client, server::server},
    utils::logging,
};

#[tokio::main]
async fn main() -> Result<()> {
    logging::init()?;
    let opts = Opts::parse();

    let endpoint = build_endpoint().await?;

    println!("{}", endpoint.node_id());

    match opts.command {
        Command::Server { ports } => server(endpoint, ports).await?,
        Command::Client { id, mapping } => client(endpoint, id, mapping).await?,
    }

    Ok(())
}
