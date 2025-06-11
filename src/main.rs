use clap::Parser;

use owo_colors::OwoColorize;
use punch::{
    cli::{Command, HostCommand, Opts},
    core::{build_endpoint, client::client, server::server},
    utils::{
        config::{ClientConfig, Host, load_config},
        crypto::load_secret_key,
        logging, reduced_node_id,
    },
};

#[tokio::main]
async fn main() -> miette::Result<()> {
    let opts = Opts::parse();
    run(opts).await?;
    Ok(())
}

async fn run(opts: Opts) -> punch::Result<()> {
    logging::init()?;

    let sk = load_secret_key(&opts).await?;

    let endpoint = build_endpoint(sk).await?;

    match opts.command {
        Command::Server {} => server(endpoint).await?,
        Command::Client {
            to,
            mapping,
            protocol,
        } => client(endpoint, to, mapping, protocol).await?,
        Command::Id { short } => {
            let node_id = endpoint.node_id();
            if short {
                println!("{}", reduced_node_id(&node_id));
            } else {
                println!("{}", node_id.to_string().bold().blue());
            }
        }
        Command::Hosts { command } => {
            let mut config: ClientConfig = load_config("client.toml").await?;
            match command {
                HostCommand::List { full } => {
                    if config.hosts.is_empty() {
                        println!("No hosts configured.");
                    } else {
                        for host in &config.hosts {
                            if full {
                                println!("{}: {}", host.name, host.id.bold().blue());
                            } else {
                                println!("{}: {}", host.name, reduced_node_id(&host.id));
                            }
                        }
                    }
                }
                HostCommand::Add { name, id } => {
                    if config.hosts.iter().any(|h| h.name == name) {
                        return Err(
                            anyhow::anyhow!("Host with name '{}' already exists.", name).into()
                        );
                    }
                    let id = id
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Invalid node ID format."))?;
                    let new_host = Host {
                        name: name.clone(),
                        id,
                    };
                    config.hosts.push(new_host);
                    punch::utils::config::save_config("client.toml", &config).await?;
                    punch::success!("Added host: {} ({})", name, reduced_node_id(&id));
                }
                HostCommand::Remove { identifier } => {
                    if let Some(pos) = config
                        .hosts
                        .iter()
                        .position(|h| h.name == identifier || h.id.to_string() == identifier)
                    {
                        let removed_host = config.hosts.remove(pos);
                        punch::utils::config::save_config("client.toml", &config).await?;
                        punch::success!(
                            "Removed host: {} ({})",
                            removed_host.name,
                            reduced_node_id(&removed_host.id)
                        );
                    } else {
                        return Err(anyhow::anyhow!(
                            "No host found with name or ID '{}'.",
                            identifier
                        )
                        .into());
                    }
                }
            }
        }
    }

    Ok(())
}
