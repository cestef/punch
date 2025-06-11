use std::sync::Arc;

use crate::{CloseReason, Result, cli::Protocol, utils::reduced_node_id};
use dashmap::DashMap;
use iroh::{
    Endpoint,
    endpoint::Connection,
    protocol::{ProtocolHandler, Router},
};
use n0_future::boxed::BoxFuture;
use tokio::{
    io::{AsyncRead, AsyncWrite, Join},
    net::TcpStream,
};

use crate::utils::{
    config::{ServerConfig, load_config},
    constants::ALPN,
};

pub async fn server(endpoint: Endpoint) -> Result<()> {
    let config: ServerConfig = load_config("server.toml").await?;
    let node_id = endpoint.node_id();
    let router = Router::builder(endpoint)
        .accept(
            ALPN,
            Tunnel {
                config: Arc::new(config),
                states: Arc::new(DashMap::new()),
            },
        )
        .spawn();

    crate::info!(
        "Server started, connect to it at: {}",
        node_id.to_string().blue().bold()
    );

    tokio::signal::ctrl_c().await?;

    router.shutdown().await?;
    Ok(())
}

#[derive(Debug, Clone)]
struct Tunnel {
    config: Arc<ServerConfig>,
    states: Arc<DashMap<iroh::NodeId, State>>,
}

#[derive(Debug, Clone)]
struct State {
    requested_port: u16,
    requested_protocol: Protocol,
}

impl ProtocolHandler for Tunnel {
    fn on_connecting(
        &self,
        connecting: iroh::endpoint::Connecting,
    ) -> BoxFuture<anyhow::Result<Connection>> {
        let config = self.config.clone();
        let states = self.states.clone();
        Box::pin(async move {
            let connecting = connecting.await?;
            let remote_node_id = connecting.remote_node_id()?;
            if !config.authorized_keys.contains(&remote_node_id) {
                tracing::warn!(
                    "Unauthorized connection attempt from node: {}",
                    reduced_node_id(&remote_node_id)
                );
                CloseReason::Unauthorized.execute(&connecting);
                return Err(anyhow::anyhow!("Unauthorized connection"));
            }
            let datagram = connecting.read_datagram().await?;
            let requested_protocol = datagram.first().ok_or(anyhow::anyhow!(
                "Failed to read requested protocol from datagram"
            ))?;
            let requested_protocol = match requested_protocol {
                0x00 => Protocol::Tcp,
                0x01 => Protocol::Udp,
                _ => {
                    tracing::warn!(
                        "Invalid protocol requested by node {}: {}",
                        reduced_node_id(&remote_node_id),
                        requested_protocol
                    );
                    CloseReason::InvalidProtocol.execute(&connecting);
                    return Err(anyhow::anyhow!("Invalid protocol requested"));
                }
            };
            tracing::info!(
                "Connection request from node: {}, requested protocol: {:?}",
                reduced_node_id(&remote_node_id),
                requested_protocol
            );

            let requested_port = connecting.read_datagram().await?; // Bytes
            let requested_port =
                u16::from_be_bytes(requested_port.iter().as_slice().try_into().map_err(|_| {
                    tracing::warn!(
                        "Failed to convert requested port bytes: {:?}",
                        requested_port
                    );
                    CloseReason::InvalidPort.execute(&connecting);
                    anyhow::anyhow!("Invalid port bytes")
                })?);

            tracing::info!(
                "Connection request from node: {}, requested port: {}",
                reduced_node_id(&remote_node_id),
                requested_port
            );

            if requested_port < 1024 {
                // >65535 can not be reached as we use u16
                tracing::warn!(
                    "Invalid port requested by node {}: {}",
                    reduced_node_id(&remote_node_id),
                    requested_port
                );
                CloseReason::InvalidPort.execute(&connecting);
                return Err(anyhow::anyhow!("Invalid port requested"));
            }

            tracing::info!("Connecting to node: {}", reduced_node_id(&remote_node_id));

            let state = State {
                requested_port,
                requested_protocol,
            };
            states.insert(remote_node_id, state);
            Ok(connecting)
        })
    }

    fn accept(&self, tunnel_conn: Connection) -> BoxFuture<anyhow::Result<()>> {
        let _config = self.config.clone();
        let states = self.states.clone();
        Box::pin(async move {
            let client_node_id = tunnel_conn.remote_node_id()?;
            tracing::info!(
                "Accepted tunnel connection from node: {}",
                reduced_node_id(&client_node_id)
            );

            loop {
                let states = states.clone();
                tokio::select! {
                    biased;

                    // Wait for the connection to close
                    _ = tunnel_conn.closed() => {
                        tracing::info!("Tunnel connection closed for node: {}", reduced_node_id(&client_node_id));
                        break;
                    }

                    stream = tunnel_conn.accept_uni() => {
                        match stream {
                            Ok(tunnel_stream) => {
                                tokio::spawn(async move {
                                    let state = states.get(&client_node_id).ok_or_else(|| {
                                        tracing::warn!(
                                            "State for node {} not found",
                                            reduced_node_id(&client_node_id)
                                        );
                                        anyhow::anyhow!("State not found")
                                    })?;
                                    tracing::info!(
                                        "Handling unidirectional stream for node: {}, requested port: {}",
                                        reduced_node_id(&client_node_id),
                                        state.requested_port
                                    );
                                    if let Err(e) = handle_uni_stream(
                                        tunnel_stream,
                                        state.value(),
                                    )
                                    .await
                                    {
                                        tracing::error!("Error handling unidirectional stream: {}", e);
                                    }
                                    anyhow::Ok(())
                                });
                            }
                            Err(e) => {
                                tracing::info!("Connection closed: {}", e);
                                break;
                            }
                        }
                        continue;
                    }

                    stream = tunnel_conn.accept_bi() => {
                        match stream {
                            Ok((tunnel_send, tunnel_recv)) => {
                                tokio::spawn(async move {
                                    let state = states.get(&client_node_id).ok_or_else(|| {
                                        tracing::warn!(
                                            "State for node {} not found",
                                            reduced_node_id(&client_node_id)
                                        );
                                        anyhow::anyhow!("State not found")
                                    })?;
                                    tracing::info!(
                                        "Handling stream for node: {}, requested port: {}",
                                        reduced_node_id(&client_node_id),
                                        state.requested_port
                                    );
                                    if let Err(e) = handle_bi_stream(
                                        tokio::io::join(tunnel_recv, tunnel_send),
                                        state.value(),
                                    )
                                    .await
                                    {
                                        tracing::error!("Error handling stream: {}", e);
                                    }
                                    anyhow::Ok(())
                                });
                            }
                            Err(e) => {
                                tracing::info!("Connection closed: {}", e);
                                break;
                            }
                        }
                    }
                }
            }

            Ok(())
        })
    }
}

async fn handle_bi_stream(
    mut tunnel_stream: Join<impl AsyncRead + Unpin, impl AsyncWrite + Unpin>,
    state: &State,
) -> Result<()> {
    match state.requested_protocol {
        Protocol::Tcp => {
            let mut local_tcp_conn =
                TcpStream::connect(format!("127.0.0.1:{}", state.requested_port)).await?;

            tokio::io::copy_bidirectional(&mut tunnel_stream, &mut local_tcp_conn).await?;
        }
        Protocol::Udp => {}
    }

    tracing::info!("Stream for port {} closed", state.requested_port);
    Ok(())
}

async fn handle_uni_stream(mut tunnel_stream: impl AsyncRead + Unpin, state: &State) -> Result<()> {
    match state.requested_protocol {
        Protocol::Tcp => {}
        Protocol::Udp => {
            use tokio::net::UdpSocket;

            let socket = UdpSocket::bind("127.0.0.1:0").await?; // Bind to a random port
            socket
                .connect(format!("127.0.0.1:{}", state.requested_port))
                .await?;
            use tokio::io::AsyncReadExt;
            let mut buf = [0; 65536]; // 64KB buffer for UDP packets
            while let Ok(size) = tunnel_stream.read(&mut buf).await {
                if size == 0 {
                    break; // EOF
                }
                let data = &mut buf[..size];
                tracing::debug!("Received {} bytes from tunnel stream for UDP", size);
                if let Err(e) = socket.send(data).await {
                    tracing::error!("Failed to send UDP packet: {}", e);
                    break;
                }
            }
        }
    }
    tracing::info!(
        "Unidirectional stream for port {} closed",
        state.requested_port
    );
    Ok(())
}
