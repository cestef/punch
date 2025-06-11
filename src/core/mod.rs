use anyhow::Result;
use iroh::{Endpoint, SecretKey};

pub mod client;
pub mod server;

pub async fn build_endpoint(sk: SecretKey) -> Result<Endpoint> {
    Endpoint::builder()
        .discovery_n0()
        .discovery_local_network()
        .secret_key(sk)
        .bind()
        .await
}
