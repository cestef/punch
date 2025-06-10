use std::sync::Arc;

use anyhow::Result;
use iroh::{
    Endpoint,
    endpoint::{Connection, RecvStream, SendStream, VarInt},
    protocol::{ProtocolHandler, Router},
};
use n0_future::boxed::BoxFuture;
use tokio::{io::AsyncReadExt, net::TcpStream};

use crate::ALPN;

pub async fn server(endpoint: Endpoint, allowed_ports: Vec<u16>) -> Result<()> {
    let router = Router::builder(endpoint)
        .accept(
            ALPN,
            Tunnel {
                allowed_ports: Arc::new(allowed_ports),
            },
        )
        .spawn();

    tokio::signal::ctrl_c().await?;

    router.shutdown().await?;
    Ok(())
}

#[derive(Debug, Clone)]
struct Tunnel {
    allowed_ports: Arc<Vec<u16>>,
}

impl ProtocolHandler for Tunnel {
    fn accept(&self, tunnel_conn: Connection) -> BoxFuture<Result<()>> {
        let allowed_ports = self.allowed_ports.clone();
        Box::pin(async move {
            let client_node_id = tunnel_conn.remote_node_id()?;
            tracing::info!("Accepted tunnel connection from node: {}", client_node_id);

            loop {
                match tunnel_conn.accept_bi().await {
                    Ok((tunnel_send, tunnel_recv)) => {
                        let allowed_ports = allowed_ports.clone();
                        let tunnel_conn = tunnel_conn.clone();

                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_stream(tunnel_send, tunnel_recv, &allowed_ports, tunnel_conn)
                                    .await
                            {
                                tracing::error!("Error handling stream: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::info!("Connection closed: {}", e);
                        break;
                    }
                }
            }

            Ok(())
        })
    }
}

async fn handle_stream(
    tunnel_send: SendStream,
    tunnel_recv: RecvStream,
    allowed_ports: &Vec<u16>,
    tunnel_conn: Connection,
) -> Result<()> {
    let mut tunnel_stream = tokio::io::join(tunnel_recv, tunnel_send);

    let requested_port = tunnel_stream.read_u16().await?;
    tracing::info!("Requested port: {}", requested_port);

    if !allowed_ports.contains(&requested_port) {
        tracing::warn!("Requested port {} is not allowed", requested_port);
        tunnel_conn.close(VarInt::from_u32(403), b"Port not allowed");
        return Ok(());
    }

    let mut local_tcp_conn = TcpStream::connect(format!("127.0.0.1:{}", requested_port)).await?;

    #[cfg(feature = "splice")]
    tokio_splice::zero_copy_bidirectional(&mut tunnel_stream, &mut local_tcp_conn).await?;
    #[cfg(not(feature = "splice"))]
    tokio::io::copy_bidirectional(&mut tunnel_stream, &mut local_tcp_conn).await?;

    tracing::info!("Stream for port {} closed", requested_port);
    Ok(())
}
