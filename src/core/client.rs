use crate::ALPN;
use anyhow::Result;
use iroh::{Endpoint, NodeId};
use tokio::{io::AsyncWriteExt, net::TcpListener};

pub async fn client(
    endpoint: Endpoint,
    node_id: NodeId,
    (local_port, remote_port): (u16, u16),
) -> Result<()> {
    let tunnel_conn = connect_with_retries(&endpoint, node_id, ALPN).await?;

    let local_listener = TcpListener::bind(format!("127.0.0.1:{}", local_port)).await?;
    while let Ok((mut local_tcp_conn, client_addr)) = local_listener.accept().await {
        let (tunnel_send, tunnel_recv) = tunnel_conn.open_bi().await?;
        let mut tunnel_stream = tokio::io::join(tunnel_recv, tunnel_send);

        tunnel_stream.write_u16(remote_port).await?;

        tracing::info!("Accepted connection from {}", client_addr);
        tokio::spawn(async move {
            #[cfg(feature = "splice")]
            tokio_splice::zero_copy_bidirectional(&mut tunnel_stream, &mut local_tcp_conn).await?;
            #[cfg(not(feature = "splice"))]
            tokio::io::copy_bidirectional(&mut tunnel_stream, &mut local_tcp_conn).await?;

            anyhow::Ok(())
        });
    }

    tunnel_conn.closed().await;

    Ok(())
}

const MAX_RETRIES: usize = 5;

async fn connect_with_retries(
    endpoint: &Endpoint,
    node_id: NodeId,
    alpn: &[u8],
) -> Result<iroh::endpoint::Connection> {
    let mut retries = 0;
    loop {
        match endpoint.connect(node_id, alpn).await {
            Ok(conn) => return Ok(conn),
            Err(e) if retries < MAX_RETRIES => {
                retries += 1;
                tracing::warn!("Connection failed, retrying... ({})", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
