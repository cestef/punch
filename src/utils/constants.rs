pub const ALPN: &[u8] = b"punch/0";
pub const MAX_RETRIES: usize = 5;

pub const PRIVATE_KEY_PATH: &str = "private_key";

pub const DEFAULT_TIMEOUT: u64 = 30; // seconds
pub const DEFAULT_RETRIES: usize = 5;
pub const DEFAULT_MAX_CONNECTIONS: usize = 100;
pub const DEFAULT_ALLOWED_PORT_RANGE: (u16, u16) = (1024, 65535);
