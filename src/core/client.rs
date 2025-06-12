use crate::core::{Protocol, TunnelConnection};
use crate::utils::config::{ClientConfig, Host, load_config, save_config};
use crate::utils::constants::{ALPN, MAX_RETRIES};
use crate::utils::reduced_node_id;
use crate::{CloseReason, PunchError, Result};
use inquire::validator::Validation;
use iroh::{Endpoint, NodeId};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};
use tokio::time::{Duration, sleep};

pub struct Client {
    endpoint: Endpoint,
    config: ClientConfig,
}

impl Client {
    pub async fn new(endpoint: Endpoint) -> Result<Self> {
        Ok(Self {
            endpoint,
            config: load_config().await?,
        })
    }

    pub async fn connect(
        mut self,
        target: String,
        local_port: u16,
        remote_port: u16,
        protocol: Protocol,
    ) -> Result<()> {
        let node_id = self.resolve_node_id(&target).await?;

        crate::info!("Connecting to node {}", reduced_node_id(&node_id));

        let connection = self
            .establish_connection(node_id, remote_port, protocol)
            .await?;

        crate::success!(
            "Connected to node {} on remote port {}",
            reduced_node_id(&node_id),
            remote_port.green().bold()
        );

        let tunnel = TunnelConnection::new(connection, protocol);
        self.handle_local_connections(tunnel, local_port).await
    }

    async fn resolve_node_id(&mut self, target: &str) -> Result<NodeId> {
        // Check if it's a known host name
        if let Some(host) = self.config.hosts.iter().find(|h| h.name == target) {
            return Ok(host.id);
        }

        // Try to parse as NodeId
        if let Ok(node_id) = target.parse::<NodeId>() {
            // Check if we already know this NodeId
            if self.config.hosts.iter().any(|h| h.id == node_id) {
                return Ok(node_id);
            }

            // Ask to add new host
            if self.prompt_add_host(&node_id).await? {
                self.add_host_interactive(node_id).await?;
            }
            return Ok(node_id);
        }

        Err(anyhow::anyhow!("Invalid node ID or host name: {}", target).into())
    }

    async fn prompt_add_host(&self, node_id: &NodeId) -> Result<bool> {
        inquire::Confirm::new(&format!(
            "Connecting to node ID {}. Add it to known hosts?",
            reduced_node_id(node_id)
        ))
        .with_default(true)
        .prompt()
        .map_err(Into::into)
    }

    async fn add_host_interactive(&mut self, node_id: NodeId) -> Result<()> {
        let hosts = self.config.hosts.clone();
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
            id: node_id,
            added_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            description: None,
            last_connected: None,
        };
        self.config.hosts.push(new_host);
        save_config(&self.config).await?;
        Ok(())
    }

    async fn establish_connection(
        &self,
        node_id: NodeId,
        remote_port: u16,
        protocol: Protocol,
    ) -> Result<iroh::endpoint::Connection> {
        let mut retries = 0;

        loop {
            match self.try_connect(node_id, remote_port, protocol).await {
                Ok(conn) => return Ok(conn),
                Err(PunchError::ConnectionClosed { reason }) => {
                    tracing::error!("Connection closed by remote peer: {}", reason);
                    return Err(PunchError::ConnectionClosed { reason });
                }
                Err(e) if retries < MAX_RETRIES => {
                    retries += 1;
                    tracing::warn!("Connection failed, retrying... ({})", e);
                    sleep(Duration::from_secs(1)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn try_connect(
        &self,
        node_id: NodeId,
        remote_port: u16,
        protocol: Protocol,
    ) -> Result<iroh::endpoint::Connection> {
        let conn = self.endpoint.connect(node_id, ALPN).await?;

        // Send protocol and port information
        conn.send_datagram(bytes::Bytes::from(vec![protocol as u8]))?;
        conn.send_datagram(bytes::Bytes::copy_from_slice(&remote_port.to_be_bytes()))?;

        // Wait a bit to see if connection gets closed immediately (authorization failure)
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                // Connection stayed open, likely authorized
                Ok(conn)
            }
            _ = conn.closed() => {
                match conn.close_reason() {
                    Some(iroh::endpoint::ConnectionError::ApplicationClosed(e)) => {
                        let close_reason: CloseReason = e.error_code.into();
                        Err(PunchError::ConnectionClosed { reason: close_reason })

                    }
                    Some(e) => Err(crate::error!("Connection closed unexpectedly: {}", e)),
                    None => Err(PunchError::ConnectionClosed {
                        reason: CloseReason::Unknown,
                    }),
                }
            }
        }
    }

    async fn handle_local_connections(
        &self,
        tunnel: TunnelConnection,
        local_port: u16,
    ) -> Result<()> {
        let local_addr: SocketAddr = ([127, 0, 0, 1], local_port).into();

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Setup graceful shutdown on Ctrl+C
        let shutdown_signal = shutdown_tx.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            let _ = shutdown_signal.send(true);
        });

        match tunnel.protocol() {
            Protocol::Tcp => {
                self.handle_tcp_connections_with_shutdown(tunnel, local_addr, shutdown_rx)
                    .await
            }
            Protocol::Udp => {
                self.handle_udp_connections_with_shutdown(tunnel, local_addr, shutdown_rx)
                    .await
            }
        }
    }

    async fn handle_tcp_connections_with_shutdown(
        &self,
        tunnel: TunnelConnection,
        local_addr: SocketAddr,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        let listener = TcpListener::bind(local_addr).await?;

        crate::info!(
            "Listening for TCP connections on {}",
            format!("{}", local_addr.green()).bold()
        );

        let tunnel = Arc::new(tunnel);
        let (tunnel_shutdown_tx, mut tunnel_shutdown_rx) = tokio::sync::watch::channel(false);

        // Monitor tunnel connection status
        let tunnel_monitor = Arc::clone(&tunnel);
        let shutdown_monitor = tunnel_shutdown_tx.clone();
        tokio::spawn(async move {
            tunnel_monitor.wait_closed().await;
            let _ = shutdown_monitor.send(true);
        });

        loop {
            tokio::select! {
                // Check for Ctrl+C shutdown
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        crate::info!("Shutting down client...");
                        break;
                    }
                }

                // Check for tunnel closure
                _ = tunnel_shutdown_rx.changed() => {
                    if *tunnel_shutdown_rx.borrow() {
                        crate::warning!("Tunnel connection closed");
                        break;
                    }
                }

                // Accept new connections
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, client_addr)) => {
                            let tunnel = Arc::clone(&tunnel);
                            let mut shutdown_rx = shutdown_rx.clone();
                            let mut tunnel_shutdown_rx = tunnel_shutdown_rx.clone();

                            tokio::spawn(async move {
                                tracing::debug!("Accepted connection from {}", client_addr);

                                tokio::select! {
                                    result = tunnel.handle_tcp_stream(stream) => {
                                        if let Err(e) = result {
                                            tracing::error!("Error handling TCP stream: {}", e);
                                        }
                                    }
                                    _ = shutdown_rx.changed() => {
                                        tracing::debug!("Closing TCP stream due to shutdown");
                                    }
                                    _ = tunnel_shutdown_rx.changed() => {
                                        tracing::debug!("Closing TCP stream due to tunnel shutdown");
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("Failed to accept connection: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_udp_connections_with_shutdown(
        &self,
        tunnel: TunnelConnection,
        local_addr: SocketAddr,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        let socket = UdpSocket::bind(local_addr).await?;

        crate::info!(
            "Listening for UDP packets on {}",
            format!("{}", local_addr.green()).bold()
        );

        tokio::select! {
            result = tunnel.handle_udp_socket(socket) => {
                Ok(result?)
            }
            _ = tunnel.wait_closed() => {
                crate::warning!("Tunnel connection closed");
                Ok(())
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    crate::info!("Shutting down client...");
                    Ok(())
                } else {
                    Ok(())
                }
            }
        }
    }
}

pub async fn client(
    endpoint: Endpoint,
    connect_to: String,
    (local_port, remote_port): (u16, u16),
    protocol: Protocol,
) -> Result<()> {
    let client = Client::new(endpoint).await?;
    client
        .connect(connect_to, local_port, remote_port, protocol)
        .await
}
