use crate::Result;
use iroh::{Endpoint, SecretKey, endpoint::Connection};
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::net::{TcpStream, UdpSocket};

pub mod client;
pub mod server;

pub async fn build_endpoint(sk: SecretKey) -> Result<Endpoint> {
    Ok(Endpoint::builder()
        .discovery_n0()
        .discovery_local_network()
        .secret_key(sk)
        .bind()
        .await?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Protocol {
    Tcp = 0x0,
    Udp = 0x1,
}

impl TryFrom<u8> for Protocol {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x0 => Ok(Protocol::Tcp),
            0x1 => Ok(Protocol::Udp),
            _ => Err("Invalid protocol byte. Use 0x0 for TCP or 0x1 for UDP.".to_string()),
        }
    }
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

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Tcp => write!(f, "TCP"),
            Protocol::Udp => write!(f, "UDP"),
        }
    }
}

pub struct TunnelConnection {
    conn: Connection,
    protocol: Protocol,
}

impl TunnelConnection {
    pub fn new(conn: Connection, protocol: Protocol) -> Self {
        Self { conn, protocol }
    }

    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    pub async fn wait_closed(&self) {
        self.conn.closed().await;
    }

    pub async fn handle_tcp_stream(&self, mut local_stream: TcpStream) -> Result<()> {
        let (tunnel_send, tunnel_recv) = self.conn.open_bi().await?;
        let mut tunnel_stream = tokio::io::join(tunnel_recv, tunnel_send);

        tokio::io::copy_bidirectional(&mut tunnel_stream, &mut local_stream).await?;
        Ok(())
    }

    pub async fn handle_udp_socket(&self, socket: UdpSocket) -> Result<()> {
        let mut tunnel_stream = self.conn.open_uni().await?;
        let mut buf = vec![0u8; 64 * 1024]; // 64KB

        loop {
            tokio::select! {
                _ = self.conn.closed() => {
                    tracing::debug!("UDP tunnel connection closed");
                    break;
                }

                result = socket.recv_from(&mut buf) => {
                    match result {
                        Ok((size, client_addr)) => {
                            tracing::debug!("Received {} bytes from {}", size, client_addr);

                            if size > self.conn.datagram_send_buffer_space() {
                                tracing::warn!("Packet too large for tunnel: {} bytes", size);
                                continue;
                            }

                            if let Err(e) = tunnel_stream.write_all(&buf[..size]).await {
                                tracing::error!("Failed to send UDP packet through tunnel: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Error receiving UDP packet: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn accept_streams(&self) -> Result<()> {
        loop {
            tokio::select! {
                biased;

                _ = self.conn.closed() => {
                    tracing::info!("Tunnel connection closed");
                    break;
                }

                result = self.conn.accept_bi() => {
                    match result {
                        Ok((send, recv)) => {
                            let handler = ConnectionHandler::new(0, self.protocol);
                            tokio::spawn(async move {
                                if let Err(e) = handler.handle_bidirectional_stream(send, recv).await {
                                    tracing::error!("Error handling bidirectional stream: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            tracing::info!("Connection closed: {}", e);
                            break;
                        }
                    }
                }

                result = self.conn.accept_uni() => {
                    match result {
                        Ok(stream) => {
                            let handler = ConnectionHandler::new(0, self.protocol);
                            tokio::spawn(async move {
                                if let Err(e) = handler.handle_unidirectional_stream(stream).await {
                                    tracing::error!("Error handling unidirectional stream: {}", e);
                                }
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
    }
}

pub struct ConnectionHandler {
    port: u16,
    protocol: Protocol,
}

impl ConnectionHandler {
    pub fn new(port: u16, protocol: Protocol) -> Self {
        Self { port, protocol }
    }

    pub async fn handle_connection(&self, tunnel: TunnelConnection) -> Result<()> {
        match self.protocol {
            Protocol::Tcp => self.handle_tcp_tunnel(tunnel).await,
            Protocol::Udp => self.handle_udp_tunnel(tunnel).await,
        }
    }

    async fn handle_tcp_tunnel(&self, tunnel: TunnelConnection) -> Result<()> {
        loop {
            tokio::select! {
                biased;

                _ = tunnel.conn.closed() => {
                    tracing::info!("TCP tunnel closed");
                    break;
                }

                result = tunnel.conn.accept_bi() => {
                    match result {
                        Ok((send, recv)) => {
                            let port = self.port;
                            tokio::spawn(async move {
                                if let Err(e) = Self::bridge_tcp_streams(send, recv, port).await {
                                    tracing::error!("Error bridging TCP streams: {}", e);
                                }
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
    }

    async fn handle_udp_tunnel(&self, tunnel: TunnelConnection) -> Result<()> {
        loop {
            tokio::select! {
                biased;

                _ = tunnel.conn.closed() => {
                    tracing::info!("UDP tunnel closed");
                    break;
                }

                result = tunnel.conn.accept_uni() => {
                    match result {
                        Ok(stream) => {
                            let port = self.port;
                            tokio::spawn(async move {
                                if let Err(e) = Self::forward_udp_packets(stream, port).await {
                                    tracing::error!("Error forwarding UDP packets: {}", e);
                                }
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
    }

    async fn bridge_tcp_streams(
        send: impl AsyncWrite + Unpin,
        recv: impl AsyncRead + Unpin,
        port: u16,
    ) -> Result<()> {
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        let mut local_stream = TcpStream::connect(addr).await?;
        let mut tunnel_stream = tokio::io::join(recv, send);

        tokio::io::copy_bidirectional(&mut tunnel_stream, &mut local_stream).await?;

        tracing::info!("TCP stream for port {} closed", port);
        Ok(())
    }

    async fn forward_udp_packets(
        mut tunnel_stream: impl AsyncRead + Unpin,
        port: u16,
    ) -> Result<()> {
        let socket = UdpSocket::bind("127.0.0.1:0").await?;
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        socket.connect(addr).await?;

        let mut buf = vec![0u8; 65536];

        loop {
            match tunnel_stream.read(&mut buf).await {
                Ok(0) => break,
                Ok(size) => {
                    tracing::debug!("Forwarding {} bytes to UDP port {}", size, port);
                    if let Err(e) = socket.send(&buf[..size]).await {
                        tracing::error!("Failed to send UDP packet: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Error reading from tunnel: {}", e);
                    break;
                }
            }
        }

        tracing::info!("UDP stream for port {} closed", port);
        Ok(())
    }

    pub async fn handle_bidirectional_stream(
        &self,
        send: impl AsyncWrite + Unpin,
        recv: impl AsyncRead + Unpin,
    ) -> Result<()> {
        match self.protocol {
            Protocol::Tcp => Self::bridge_tcp_streams(send, recv, self.port).await,
            Protocol::Udp => Err(crate::error!("Bidirectional UDP streams are not supported")),
        }
    }

    pub async fn handle_unidirectional_stream(&self, stream: impl AsyncRead + Unpin) -> Result<()> {
        match self.protocol {
            Protocol::Tcp => Err(crate::error!(
                "Unidirectional TCP streams are not supported"
            )),
            Protocol::Udp => Self::forward_udp_packets(stream, self.port).await,
        }
    }
}
