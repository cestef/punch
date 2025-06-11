use crate::Result;
use crate::cli::Protocol;
use crate::utils::config::{ClientConfig, Host, load_config, save_config};
use crate::utils::constants::{ALPN, MAX_RETRIES};
use crate::utils::reduced_node_id;
use inquire::validator::Validation;
use iroh::{Endpoint, NodeId};
use tokio::net::{TcpListener, UdpSocket};

pub async fn client(
    endpoint: Endpoint,
    connect_to: String,
    (local_port, remote_port): (u16, u16),
    protocol: Protocol,
) -> Result<()> {
    let mut config: ClientConfig = load_config("client.toml").await?;
    let node_id = if let Some(host) = config.hosts.iter().find(|h| h.name == connect_to) {
        host.id
    } else if let Ok(node_id) = connect_to.parse::<NodeId>() {
        // Check if it matches any known host by ID
        if let Some(host) = config.hosts.iter().find(|h| h.id == node_id) {
            host.id
        } else if inquire::Confirm::new(&format!(
            "Connecting to node ID {}. Add it to known hosts?",
            reduced_node_id(&node_id)
        ))
        .with_default(true)
        .prompt()?
        {
            let hosts = config.hosts.clone();
            let name = inquire::Text::new("Enter a name for this host:")
                .with_validator(move |input: &str| {
                    if input.is_empty() {
                        Ok(Validation::Invalid("Host name cannot be empty.".into()))
                    } else if hosts.iter().any(|h| h.name == input) {
                        Ok(Validation::Invalid(
                            "Host name already exists. Please choose a different name.".into(),
                        ))
                    } else {
                        Ok(Validation::Valid)
                    }
                })
                .prompt()?;
            let new_host = Host {
                name,
                id: node_id.clone(),
            };
            config.hosts.push(new_host);
            save_config("client.toml", &config).await?;
            node_id
        } else {
            node_id
        }
    } else {
        return Err(anyhow::anyhow!("Invalid node ID or host name: {}", connect_to).into());
    };

    crate::info!("Connecting to node {}", reduced_node_id(&node_id));

    let tunnel_conn = connect_with_retries(&endpoint, node_id, ALPN, remote_port, protocol).await?;
    crate::success!(
        "Connected to node {} on remote port {}",
        reduced_node_id(&node_id),
        remote_port.green().bold()
    );
    match protocol {
        Protocol::Tcp => {
            let local_listener = TcpListener::bind(format!("127.0.0.1:{}", local_port)).await?;

            crate::info!(
                "Listening for TCP connections on {}",
                format!("127.0.0.1:{}", local_port.green()).bold()
            );

            while let Ok((mut local_tcp_conn, client_addr)) = local_listener.accept().await {
                tracing::debug!("Accepted connection from {}", client_addr);
                let (tunnel_send, tunnel_recv) = tunnel_conn.open_bi().await?;
                tracing::debug!(
                    "Opened bidirectional tunnel to {}",
                    reduced_node_id(&node_id)
                );
                let mut tunnel_stream = tokio::io::join(tunnel_recv, tunnel_send);

                tokio::spawn(async move {
                    anyhow::Ok(
                        tokio::io::copy_bidirectional(&mut tunnel_stream, &mut local_tcp_conn)
                            .await?,
                    )
                });
            }
        }
        Protocol::Udp => {
            let socket = UdpSocket::bind(format!("127.0.0.1:{}", local_port)).await?;
            let mut tunnel_stream = tunnel_conn.open_uni().await?;
            crate::info!(
                "Listening for UDP packets on {}",
                format!("127.0.0.1:{}", local_port.green()).bold()
            );

            let mut buf = [0; 65536]; // 64KB buffer for UDP packets
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((size, client_addr)) => {
                        tracing::debug!("Received {} bytes from {}", size, client_addr);
                        let data = &buf[..size];

                        if size > tunnel_conn.datagram_send_buffer_space() {
                            tracing::warn!("Received packet too large for tunnel: {} bytes", size);
                            continue;
                        }
                        if let Err(e) = tunnel_stream.write(data).await {
                            tracing::error!("Failed to send UDP packet through tunnel: {}", e);
                            continue;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error receiving UDP packet: {}", e);
                    }
                }
            }
        }
    }

    tunnel_conn.closed().await;

    Ok(())
}

async fn connect_with_retries(
    endpoint: &Endpoint,
    node_id: NodeId,
    alpn: &[u8],
    requested_port: u16,
    protocol: Protocol,
) -> Result<iroh::endpoint::Connection> {
    let mut retries = 0;

    loop {
        match endpoint.connect(node_id, alpn).await {
            Ok(conn) => {
                conn.send_datagram(bytes::Bytes::copy_from_slice(&[protocol as u8]))?;
                let port_bytes = requested_port.to_be_bytes();

                conn.send_datagram(bytes::Bytes::copy_from_slice(&port_bytes))?;
                return Ok(conn);
            }
            Err(e) if retries < MAX_RETRIES => {
                retries += 1;
                tracing::warn!("Connection failed, retrying... ({})", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}
