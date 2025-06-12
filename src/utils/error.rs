use std::path::PathBuf;

use iroh::endpoint::{Connection, VarInt};
use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum PunchError {
    #[error(transparent)]
    #[diagnostic(code(punch::other))]
    Other(#[from] anyhow::Error),

    #[error(transparent)]
    #[diagnostic(code(punch::io))]
    Io(#[from] std::io::Error),

    #[error("Config error: {path}")]
    #[diagnostic(code(punch::config_error))]
    ConfigError {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error(transparent)]
    #[diagnostic(code(punch::toml::de))]
    TomlDe(#[from] toml::de::Error),

    #[error(transparent)]
    #[diagnostic(code(punch::toml::ser))]
    TomlSer(#[from] toml::ser::Error),

    #[error(transparent)]
    #[diagnostic(code(punch::connection))]
    Connection(#[from] iroh::endpoint::ConnectionError),

    #[error("Connection closed by remote peer: {reason}")]
    #[diagnostic(code(punch::connection_closed))]
    ConnectionClosed { reason: CloseReason },

    #[error(transparent)]
    Inquire(#[from] inquire::InquireError),

    #[error(transparent)]
    Datagram(#[from] iroh::endpoint::SendDatagramError),

    #[error("Error: {message}")]
    #[diagnostic(code(punch::error))]
    Error {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Unauthorized,
    InvalidPort,
    InvalidProtocol,
    Unknown,
}

impl Into<VarInt> for &CloseReason {
    fn into(self) -> VarInt {
        match self {
            CloseReason::Unauthorized => VarInt::from(0x01 as u8),
            CloseReason::InvalidPort => VarInt::from(0x02 as u8),
            CloseReason::InvalidProtocol => VarInt::from(0x03 as u8),
            CloseReason::Unknown => VarInt::from(u8::MAX), // Use a sentinel value for unknown
        }
    }
}

impl From<VarInt> for CloseReason {
    fn from(value: VarInt) -> Self {
        match value.into_inner() {
            0x01 => CloseReason::Unauthorized,
            0x02 => CloseReason::InvalidPort,
            0x03 => CloseReason::InvalidProtocol,
            _ => panic!("Unknown CloseReason: {}", value),
        }
    }
}

impl std::fmt::Display for CloseReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloseReason::Unauthorized => write!(f, "Unauthorized connection attempt"),
            CloseReason::InvalidPort => {
                write!(f, "Invalid port requested, must be between 1024 and 65535")
            }
            CloseReason::InvalidProtocol => {
                write!(f, "Invalid protocol requested, must be TCP or UDP")
            }
            CloseReason::Unknown => write!(f, "Unknown close reason"),
        }
    }
}

impl CloseReason {
    pub fn execute(&self, connection: &Connection) {
        connection.close(self.into(), self.to_string().as_bytes())
    }
}

pub type Result<T, E = PunchError> = std::result::Result<T, E>;

#[macro_export]
macro_rules! error {
    (source = $source:expr, $($arg:tt)*) => {
        {
            crate::utils::error::PunchError::Error {
                message: format!($($arg)*),
                source: Some(Box::new($source)),
            }
        }
    };
    ($($arg:tt)*) => {
        {
            crate::utils::error::PunchError::Error {
                message: format!($($arg)*),
                source: None,
            }
        }
    };
}
