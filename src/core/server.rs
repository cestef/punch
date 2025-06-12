use crate::utils::{
    config::{AuthorizationManager, ConfigManager, ServerConfig},
    constants::ALPN,
    reduced_node_id,
};
use crate::{
    CloseReason, Result,
    core::{ConnectionHandler, Protocol, TunnelConnection},
};
use dashmap::DashMap;
use iroh::{
    Endpoint, NodeId,
    endpoint::Connection,
    protocol::{ProtocolHandler, Router},
};
use n0_future::boxed::BoxFuture;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Debug)]
pub struct Server {
    config_manager: Arc<ConfigManager>,
    auth_manager: Arc<AuthorizationManager>,
    connections: Arc<DashMap<NodeId, ConnectionState>>,
    active_connections: Arc<AtomicUsize>,
}

#[derive(Debug, Clone)]
struct ConnectionState {
    port: u16,
    protocol: Protocol,
}

impl Server {
    pub async fn new() -> Result<Self> {
        let config_manager = Arc::new(ConfigManager::new()?);
        let auth_manager = Arc::new(AuthorizationManager::new((*config_manager).clone()));

        Ok(Self {
            config_manager,
            auth_manager,
            connections: Arc::new(DashMap::new()),
            active_connections: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub async fn start(self, endpoint: Endpoint) -> Result<()> {
        let node_id = endpoint.node_id();

        let config: ServerConfig = self.config_manager.load().await?;

        if config.authorized_keys.is_empty() {
            crate::warning!("No authorized keys configured. No clients will be able to connect.");
            crate::info!("Add authorized keys to {}", "~/.punch/server.toml".bold());
        }

        let router = Router::builder(endpoint).accept(ALPN, self).spawn();

        crate::info!(
            "Server started, connect to it at: {}",
            node_id.to_string().blue().bold()
        );

        tokio::signal::ctrl_c().await?;

        crate::info!("Shutting down server...");
        router.shutdown().await?;

        Ok(())
    }

    async fn check_connection_limit(&self) -> Result<()> {
        let config: ServerConfig = self.config_manager.load().await?;
        let current = self.active_connections.load(Ordering::Relaxed);

        if current >= config.settings.max_connections {
            return Err(anyhow::anyhow!(
                "Maximum connections ({}) reached",
                config.settings.max_connections
            )
            .into());
        }

        Ok(())
    }

    async fn validate_connection(&self, conn: &Connection) -> Result<ConnectionState> {
        let remote_node_id = conn.remote_node_id()?;

        if !self.auth_manager.is_authorized(&remote_node_id).await? {
            crate::warning!(
                "Unauthorized connection attempt from node: {}",
                reduced_node_id(&remote_node_id)
            );
            CloseReason::Unauthorized.execute(conn);
            return Err(anyhow::anyhow!("Unauthorized connection").into());
        }

        self.check_connection_limit().await?;

        let protocol = self.read_protocol(conn).await?;

        let port = self.read_port(conn).await?;

        if !self.auth_manager.is_port_allowed(port).await? {
            crate::warning!(
                "Invalid port requested by node {}: {}",
                reduced_node_id(&remote_node_id),
                port
            );
            CloseReason::InvalidPort.execute(conn);
            return Err(anyhow::anyhow!("Port {} not allowed", port).into());
        }

        tracing::info!(
            "Connection request from node: {}, protocol: {:?}, port: {}",
            reduced_node_id(&remote_node_id),
            protocol,
            port
        );

        Ok(ConnectionState { port, protocol })
    }

    async fn read_protocol(&self, conn: &Connection) -> Result<Protocol> {
        let datagram = conn.read_datagram().await?;
        let first_byte = datagram
            .first()
            .ok_or_else(|| anyhow::anyhow!("Failed to read protocol from datagram"))?;

        Protocol::try_from(*first_byte).map_err(|_| {
            CloseReason::InvalidProtocol.execute(conn);
            anyhow::anyhow!("Invalid protocol requested").into()
        })
    }

    async fn read_port(&self, conn: &Connection) -> Result<u16> {
        let datagram = conn.read_datagram().await?;

        let port_bytes: [u8; 2] = datagram.iter().as_slice().try_into().map_err(|_| {
            CloseReason::InvalidPort.execute(conn);
            anyhow::anyhow!("Invalid port bytes")
        })?;

        Ok(u16::from_be_bytes(port_bytes))
    }

    async fn handle_connection(&self, conn: Connection) -> Result<()> {
        let remote_node_id = conn.remote_node_id()?;

        self.active_connections.fetch_add(1, Ordering::Relaxed);

        let _guard = ConnectionGuard {
            counter: Arc::clone(&self.active_connections),
            node_id: remote_node_id,
            connections: Arc::clone(&self.connections),
        };

        let state = self
            .connections
            .get(&remote_node_id)
            .ok_or_else(|| anyhow::anyhow!("Connection state not found"))?;

        let tunnel = TunnelConnection::new(conn, state.protocol);
        let handler = ConnectionHandler::new(state.port, state.protocol);

        tracing::info!(
            "Handling connection from node: {} on port {}",
            reduced_node_id(&remote_node_id),
            state.port
        );

        handler.handle_connection(tunnel).await?;

        Ok(())
    }
}

struct ConnectionGuard {
    counter: Arc<AtomicUsize>,
    node_id: NodeId,
    connections: Arc<DashMap<NodeId, ConnectionState>>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
        self.connections.remove(&self.node_id);
        tracing::debug!(
            "Connection closed for node: {}, active connections: {}",
            reduced_node_id(&self.node_id),
            self.counter.load(Ordering::Relaxed)
        );
    }
}

impl ProtocolHandler for Server {
    fn on_connecting(
        &self,
        connecting: iroh::endpoint::Connecting,
    ) -> BoxFuture<anyhow::Result<Connection>> {
        let server = self.clone();

        Box::pin(async move {
            let conn = connecting.await?;
            let remote_node_id = conn.remote_node_id()?;

            let state = server.validate_connection(&conn).await?;

            server.connections.insert(remote_node_id, state);

            Ok(conn)
        })
    }

    fn accept(&self, conn: Connection) -> BoxFuture<anyhow::Result<()>> {
        let server = self.clone();

        Box::pin(async move {
            let remote_node_id = conn.remote_node_id()?;

            tracing::info!(
                "Accepted tunnel connection from node: {}",
                reduced_node_id(&remote_node_id)
            );

            if let Err(e) = server.handle_connection(conn).await {
                tracing::error!(
                    "Error handling connection from {}: {}",
                    reduced_node_id(&remote_node_id),
                    e
                );
            }

            Ok(())
        })
    }
}

pub async fn server(endpoint: Endpoint) -> Result<()> {
    let server = Server::new().await?;
    server.start(endpoint).await
}
