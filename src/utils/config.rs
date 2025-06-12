use crate::Result;
use crate::utils::constants::{
    DEFAULT_ALLOWED_PORT_RANGE, DEFAULT_MAX_CONNECTIONS, DEFAULT_RETRIES, DEFAULT_TIMEOUT,
};
use iroh::{NodeId, PublicKey};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt::Debug;
use std::path::{Path, PathBuf};

pub trait Configuration: Serialize + DeserializeOwned + Debug {
    fn filename() -> &'static str;

    fn validate(&self) -> Result<()> {
        Ok(())
    }

    fn default() -> Self;
}

#[derive(Clone, Debug)]
pub struct ConfigManager {
    base_path: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Result<Self> {
        let base_path = dirs::home_dir()
            .ok_or_else(|| crate::error!("Home directory not found"))?
            .join(".punch");

        Ok(Self { base_path })
    }

    pub fn with_base_path(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    pub async fn load<C: Configuration>(&self) -> Result<C> {
        let path = self.config_path(C::filename());

        if path.exists() {
            self.load_from_file(&path).await
        } else {
            let config = C::default();
            self.save(&config).await?;
            Ok(config)
        }
    }

    pub async fn save<C: Configuration>(&self, config: &C) -> Result<()> {
        config.validate()?;

        let path = self.config_path(C::filename());
        self.ensure_directory(&path).await?;

        let content = toml::to_string_pretty(config)?;
        tokio::fs::write(&path, content)
            .await
            .map_err(|e| crate::PunchError::ConfigError {
                path: path.to_path_buf(),
                source: Box::new(e),
            })?;

        Ok(())
    }

    async fn load_from_file<C: Configuration>(&self, path: &Path) -> Result<C> {
        let content =
            tokio::fs::read_to_string(path)
                .await
                .map_err(|e| crate::PunchError::ConfigError {
                    path: path.to_path_buf(),
                    source: Box::new(e),
                })?;

        let config: C = toml::from_str(&content)?;
        config.validate()?;

        Ok(config)
    }

    fn config_path(&self, filename: &str) -> PathBuf {
        self.base_path.join(filename)
    }

    async fn ensure_directory(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                crate::PunchError::ConfigError {
                    path: parent.to_path_buf(),
                    source: Box::new(e),
                }
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    pub authorized_keys: Vec<PublicKey>,

    #[serde(default)]
    pub settings: ServerSettings,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerSettings {
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    #[serde(default = "default_port_range")]
    pub allowed_ports: (u16, u16),
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            max_connections: default_max_connections(),
            allowed_ports: default_port_range(),
        }
    }
}

fn default_max_connections() -> usize {
    DEFAULT_MAX_CONNECTIONS
}
fn default_port_range() -> (u16, u16) {
    DEFAULT_ALLOWED_PORT_RANGE
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT
}

fn default_retries() -> usize {
    DEFAULT_RETRIES
}

impl Configuration for ServerConfig {
    fn filename() -> &'static str {
        "server.toml"
    }

    fn default() -> Self {
        Self {
            authorized_keys: Vec::new(),
            settings: ServerSettings::default(),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.settings.allowed_ports.0 > self.settings.allowed_ports.1 {
            return Err(crate::error!("Invalid port range: min > max"));
        }

        if self.settings.allowed_ports.0 < 1024 {
            return Err(crate::error!("Minimum allowed port must be >= 1024"));
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClientSettings {
    #[serde(default = "default_timeout")]
    pub connection_timeout: u64,

    #[serde(default = "default_retries")]
    pub max_retries: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClientConfig {
    pub hosts: Vec<Host>,

    #[serde(default)]
    pub settings: ClientSettings,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            connection_timeout: DEFAULT_TIMEOUT,
            max_retries: DEFAULT_RETRIES,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Host {
    pub name: String,

    pub id: NodeId,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default = "current_timestamp")]
    pub added_at: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_connected: Option<u64>,
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

impl Host {
    pub fn new(name: String, id: NodeId) -> Self {
        Self {
            name,
            id,
            description: None,
            added_at: current_timestamp(),
            last_connected: None,
        }
    }

    pub fn mark_connected(&mut self) {
        self.last_connected = Some(current_timestamp());
    }
}

impl Configuration for ClientConfig {
    fn filename() -> &'static str {
        "client.toml"
    }

    fn default() -> Self {
        Self {
            hosts: Vec::new(),
            settings: ClientSettings::default(),
        }
    }

    fn validate(&self) -> Result<()> {
        let mut names = std::collections::HashSet::new();
        for host in &self.hosts {
            if !names.insert(&host.name) {
                return Err(crate::error!("Duplicate host name: {}", host.name));
            }
        }

        Ok(())
    }
}

pub struct HostManager {
    config_manager: ConfigManager,
}

impl HostManager {
    pub fn new(config_manager: ConfigManager) -> Self {
        Self { config_manager }
    }

    pub async fn add_host(
        &self,
        name: String,
        id: NodeId,
        description: Option<String>,
    ) -> Result<()> {
        let mut config: ClientConfig = self.config_manager.load().await?;

        if config.hosts.iter().any(|h| h.name == name) {
            return Err(crate::error!("Host with name '{}' already exists", name));
        }

        if let Some(existing) = config.hosts.iter().find(|h| h.id == id) {
            return Err(crate::error!(
                "Node ID already exists with name '{}'",
                existing.name
            ));
        }

        let mut host = Host::new(name, id);
        host.description = description;

        config.hosts.push(host);
        self.config_manager.save(&config).await?;

        Ok(())
    }

    pub async fn remove_host(&self, identifier: &str) -> Result<Host> {
        let mut config: ClientConfig = self.config_manager.load().await?;

        let position = config
            .hosts
            .iter()
            .position(|h| h.name == identifier || h.id.to_string() == identifier)
            .ok_or_else(|| crate::error!("Host not found: {}", identifier))?;

        let removed = config.hosts.remove(position);
        self.config_manager.save(&config).await?;

        Ok(removed)
    }

    pub async fn mark_host_connected(&self, node_id: &NodeId) -> Result<()> {
        let mut config: ClientConfig = self.config_manager.load().await?;

        if let Some(host) = config.hosts.iter_mut().find(|h| &h.id == node_id) {
            host.mark_connected();
            self.config_manager.save(&config).await?;
        }

        Ok(())
    }

    pub async fn find_host(&self, identifier: &str) -> Result<Option<Host>> {
        let config: ClientConfig = self.config_manager.load().await?;

        Ok(config
            .hosts
            .into_iter()
            .find(|h| h.name == identifier || h.id.to_string() == identifier))
    }

    pub async fn list_hosts(&self) -> Result<Vec<Host>> {
        let config: ClientConfig = self.config_manager.load().await?;
        Ok(config.hosts)
    }
}

#[derive(Clone, Debug)]
pub struct AuthorizationManager {
    config_manager: ConfigManager,
}

impl AuthorizationManager {
    pub fn new(config_manager: ConfigManager) -> Self {
        Self { config_manager }
    }

    pub async fn is_authorized(&self, node_id: &PublicKey) -> Result<bool> {
        let config: ServerConfig = self.config_manager.load().await?;
        Ok(config.authorized_keys.contains(node_id))
    }

    pub async fn authorize(&self, key: PublicKey) -> Result<()> {
        let mut config: ServerConfig = self.config_manager.load().await?;

        if !config.authorized_keys.contains(&key) {
            config.authorized_keys.push(key);
            self.config_manager.save(&config).await?;
        }

        Ok(())
    }

    pub async fn revoke(&self, key: &PublicKey) -> Result<bool> {
        let mut config: ServerConfig = self.config_manager.load().await?;

        let original_len = config.authorized_keys.len();
        config.authorized_keys.retain(|k| k != key);

        if config.authorized_keys.len() < original_len {
            self.config_manager.save(&config).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn list_authorized(&self) -> Result<Vec<PublicKey>> {
        let config: ServerConfig = self.config_manager.load().await?;
        Ok(config.authorized_keys)
    }

    pub async fn is_port_allowed(&self, port: u16) -> Result<bool> {
        let config: ServerConfig = self.config_manager.load().await?;
        let (min, max) = config.settings.allowed_ports;
        Ok(port >= min && port <= max)
    }
}

pub use self::{AuthorizationManager as Auth, ConfigManager as Manager, HostManager as Hosts};

pub async fn load_config<C: Configuration>() -> Result<C> {
    let manager = ConfigManager::new()?;
    manager.load().await
}

pub async fn save_config<C: Configuration>(config: &C) -> Result<()> {
    let manager = ConfigManager::new()?;
    manager.save(config).await
}
