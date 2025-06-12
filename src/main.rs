use clap::Parser;
use owo_colors::OwoColorize;
use punch::{
    cli::{Command, HostCommand, Opts},
    core::{build_endpoint, client::client, server::server},
    utils::{
        config::{AuthorizationManager, ConfigManager, HostManager},
        crypto::load_secret_key,
        format::format_duration,
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
    let config_manager = ConfigManager::new()?;

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
            let host_manager = HostManager::new(config_manager);
            handle_hosts_command(command, host_manager).await?;
        }
        Command::Auth { command } => {
            let auth_manager = AuthorizationManager::new(config_manager);
            handle_auth_command(command, auth_manager, endpoint.node_id()).await?;
        }
        Command::Config { show_path } => {
            if show_path {
                let path = dirs::home_dir()
                    .expect("Could not find home directory")
                    .join(".punch");
                println!("Configuration directory: {}", path.display().purple());
            } else {
                println!("Configuration files:");
                println!("  Client: ~/.punch/client.toml");
                println!("  Server: ~/.punch/server.toml");
                println!("\nUse --show-path to see the full configuration directory path");
            }
        }
    }

    Ok(())
}

async fn handle_hosts_command(
    command: HostCommand,
    host_manager: HostManager,
) -> punch::Result<()> {
    match command {
        HostCommand::List { full } => {
            let hosts = host_manager.list_hosts().await?;
            if hosts.is_empty() {
                println!("No hosts configured.");
                return Ok(());
            }

            let mut hosts = hosts;
            hosts.sort_by(|a, b| a.name.cmp(&b.name));

            for host in hosts {
                let id_display = if full {
                    host.id.to_string().bold().blue().to_string()
                } else {
                    reduced_node_id(&host.id)
                };

                print!("{}: {}", host.name.bold(), id_display);

                if let Some(desc) = &host.description {
                    print!(" - {}", desc.dimmed());
                }

                if let Some(last_connected) = host.last_connected {
                    let duration = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                        .saturating_sub(last_connected);

                    let time_ago = format_duration(duration);
                    print!(" (last connected: {})", time_ago.green());
                }

                println!();
            }
        }
        HostCommand::Add { name, id } => {
            let node_id = id
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid node ID format."))?;

            let description = inquire::Text::new("Description (optional):")
                .with_default("")
                .prompt()
                .ok()
                .filter(|s| !s.is_empty());

            host_manager
                .add_host(name.clone(), node_id, description)
                .await?;
            punch::success!("Added host: {} ({})", name, reduced_node_id(&node_id));
        }
        HostCommand::Remove { identifier } => {
            let removed_host = host_manager.remove_host(&identifier).await?;
            punch::success!(
                "Removed host: {} ({})",
                removed_host.name,
                reduced_node_id(&removed_host.id)
            );
        }
    }
    Ok(())
}

async fn handle_auth_command(
    command: punch::cli::AuthCommand,
    auth_manager: AuthorizationManager,
    our_key: iroh::PublicKey,
) -> punch::Result<()> {
    use punch::cli::AuthCommand;

    match command {
        AuthCommand::List => {
            let keys = auth_manager.list_authorized().await?;
            if keys.is_empty() {
                println!("No authorized keys configured.");
                println!(
                    "\nYour public key is: {}",
                    our_key.to_string().blue().bold()
                );
                println!("Share this with server administrators to get access.");
                return Ok(());
            }

            println!("Authorized keys:");
            for (i, key) in keys.iter().enumerate() {
                let marker = if key == &our_key {
                    " (this node)".green().to_string()
                } else {
                    "".to_string()
                };
                println!("  {}. {}{}", i + 1, key.to_string().blue(), marker);
            }
        }
        AuthCommand::Add { key } => {
            let public_key = key
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid public key format."))?;

            auth_manager.authorize(public_key).await?;
            punch::success!("Added authorized key: {}", key.blue());
        }
        AuthCommand::Remove { key } => {
            let public_key = key
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid public key format."))?;

            if auth_manager.revoke(&public_key).await? {
                punch::success!("Removed authorized key: {}", key.blue());
            } else {
                punch::warning!("Key not found in authorized list");
            }
        }
        AuthCommand::MyKey => {
            println!("Your public key: {}", our_key.to_string().blue().bold());
            println!("\nShare this key with server administrators to get access.");
        }
    }
    Ok(())
}
