//! Version 2 device addressing. Routing tickets are internal, never invitations.
use crate::{device::ComputerId, relay::RelayEndpoint};
use serde::{Deserialize, Serialize};

pub const LIMIT: usize = 4096;
#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Hello {
    Register {
        version: u8,
        public_key: [u8; 32],
        access_code: String,
        name: String,
    },
    Join {
        version: u8,
        ticket: String,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionContext {
    pub host: ComputerId,
    pub viewer: ComputerId,
    pub host_key: [u8; 32],
    pub viewer_key: [u8; 32],
    pub request: [u8; 32],
}
impl SessionContext {
    pub fn prologue(&self) -> Vec<u8> {
        let mut bytes = b"DeskLink-addressed-session-v2:".to_vec();
        bytes.extend_from_slice(self.host.as_str().as_bytes());
        bytes.extend_from_slice(self.viewer.as_str().as_bytes());
        bytes.extend_from_slice(&self.request);
        bytes
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Host,
    Viewer,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Offer {
    pub role: Role,
    pub context: SessionContext,
    pub ticket: String,
    pub name: String,
    pub control: bool,
}
#[derive(Clone)]
pub struct ConnectionOffer {
    pub(crate) offer: Offer,
    pub endpoint: RelayEndpoint,
}
impl ConnectionOffer {
    pub fn peer_id(&self) -> &ComputerId {
        if self.offer.role == Role::Host {
            &self.offer.context.viewer
        } else {
            &self.offer.context.host
        }
    }
    pub fn peer_key(&self) -> &[u8; 32] {
        if self.offer.role == Role::Host {
            &self.offer.context.viewer_key
        } else {
            &self.offer.context.host_key
        }
    }
    pub fn context(&self) -> &SessionContext {
        &self.offer.context
    }
    pub fn is_host(&self) -> bool {
        self.offer.role == Role::Host
    }
}
impl std::fmt::Debug for ConnectionOffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionOffer")
            .field("host", &self.offer.context.host)
            .field("viewer", &self.offer.context.viewer)
            .field("ticket", &"[REDACTED]")
            .finish()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Command {
    Proof {
        answer: Vec<u8>,
    },
    Request {
        target: ComputerId,
        control: bool,
        request: [u8; 32],
    },
    Cancel {
        request: [u8; 32],
    },
    CancelPending,
    Available {
        available: bool,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Event {
    Challenge { challenge: Vec<u8> },
    Registered { id: ComputerId },
    Offer(Offer),
    Paired,
    Rejected { request: [u8; 32], reason: String },
    Ended { request: [u8; 32] },
    Error { reason: String },
}
