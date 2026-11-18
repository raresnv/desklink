pub mod addressed;
mod client;
mod device_client;
pub use device_client::{join_device, DeviceEvent, DevicePresence};
mod endpoint;
pub mod server;

pub use client::{connect_viewer, register_host, PendingHost};
pub use endpoint::RelayEndpoint;

use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

pub const CHUNK: usize = 64 * 1024;
const SETUP_LIMIT: usize = 1024;

#[derive(Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase", deny_unknown_fields)]
enum Hello {
    Host { version: u8, access_code: String },
    Viewer { version: u8, room: String },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
enum Reply {
    Ready { room: String, expires_in: u64 },
    Paired,
    Error { reason: String },
}

fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .write_buffer_size(0)
        .max_write_buffer_size(2 * CHUNK)
        .max_message_size(Some(CHUNK))
        .max_frame_size(Some(CHUNK))
}

pub(crate) fn decode_capability(text: &str) -> anyhow::Result<[u8; 32]> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    anyhow::ensure!(text.len() == 43, "Invalid routing capability");
    let bytes = URL_SAFE_NO_PAD.decode(text)?;
    let key: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid routing capability"))?;
    anyhow::ensure!(key != [0; 32], "Invalid routing capability");
    Ok(key)
}
