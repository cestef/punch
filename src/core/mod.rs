use anyhow::Result;
use iroh::Endpoint;

pub mod client;
pub mod server;

pub async fn build_endpoint() -> Result<Endpoint> {
    Endpoint::builder()
        .discovery_n0()
        .discovery_local_network()
        .bind()
        .await
}
