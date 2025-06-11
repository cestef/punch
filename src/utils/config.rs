use iroh::PublicKey;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub authorized_keys: Vec<PublicKey>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ClientConfig {
    pub hosts: Vec<Host>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Host {
    pub name: String,
    pub id: iroh::NodeId,
}

pub async fn load_config<P: AsRef<std::path::Path>, C: DeserializeOwned>(
    path: P,
) -> crate::Result<C> {
    let path = dirs::home_dir()
        .ok_or_else(|| crate::error!("Home directory not found"))?
        .join(".punch")
        .join(path);
    let config_str =
        tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| crate::PunchError::ConfigError {
                path: path.to_path_buf(),
                source: Box::new(e),
            })?;
    Ok(toml::from_str(&config_str)?)
}

pub async fn save_config<P: AsRef<std::path::Path>, C: Serialize>(
    path: P,
    config: &C,
) -> crate::Result<()> {
    let path = dirs::home_dir()
        .ok_or_else(|| crate::error!("Home directory not found"))?
        .join(".punch")
        .join(path);

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| crate::PunchError::ConfigError {
                path: parent.to_path_buf(),
                source: Box::new(e),
            })?;
    }

    let config_str = toml::to_string(config)?;
    tokio::fs::write(&path, config_str)
        .await
        .map_err(|e| crate::PunchError::ConfigError {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;

    Ok(())
}
