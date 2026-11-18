use crate::relay::{decode_capability, RelayEndpoint};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};
use std::{fmt, net::SocketAddr};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, PartialEq, Eq)]
pub enum Destination {
    Direct(SocketAddr),
    Relay {
        endpoint: RelayEndpoint,
        room: [u8; 32],
    },
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Invitation {
    #[zeroize(skip)]
    pub destination: Destination,
    secret: [u8; 32],
}

impl Invitation {
    pub fn new(address: SocketAddr) -> Self {
        let mut secret = [0; 32];
        OsRng.fill_bytes(&mut secret);
        Self {
            destination: Destination::Direct(address),
            secret,
        }
    }

    pub fn secret(&self) -> &[u8; 32] {
        &self.secret
    }

    pub fn for_relay(endpoint: RelayEndpoint, room: [u8; 32]) -> Result<Self> {
        ensure!(room != [0; 32], "Invalid routing capability");
        let mut secret = [0; 32];
        OsRng.fill_bytes(&mut secret);
        Ok(Self {
            destination: Destination::Relay { endpoint, room },
            secret,
        })
    }

    pub fn direct_address(&self) -> Option<SocketAddr> {
        match self.destination {
            Destination::Direct(address) => Some(address),
            _ => None,
        }
    }

    pub fn address_label(&self) -> String {
        match &self.destination {
            Destination::Direct(address) => address.to_string(),
            Destination::Relay { endpoint, .. } => endpoint.as_str().to_owned(),
        }
    }

    pub fn encode(&self) -> String {
        match &self.destination {
            Destination::Direct(address) => format!(
                "desklink://{}#{}",
                address,
                URL_SAFE_NO_PAD.encode(self.secret)
            ),
            Destination::Relay { endpoint, room } => {
                let (scheme, authority) =
                    if let Some(rest) = endpoint.as_str().strip_prefix("wss://") {
                        ("desklink+relay://", rest)
                    } else {
                        (
                            "desklink+local-relay://",
                            endpoint
                                .as_str()
                                .strip_prefix("ws://")
                                .expect("validated endpoint"),
                        )
                    };
                format!(
                    "{scheme}{authority}#{}.{}",
                    URL_SAFE_NO_PAD.encode(room),
                    URL_SAFE_NO_PAD.encode(self.secret)
                )
            }
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        ensure!(text.len() <= 512, "Invitation is too long");
        let text = text.trim();
        for (prefix, scheme) in [
            ("desklink+relay://", "wss://"),
            ("desklink+local-relay://", "ws://"),
        ] {
            if let Some(body) = text.strip_prefix(prefix) {
                let (address, keys) = body
                    .split_once('#')
                    .context("Invitation is missing its pairing key")?;
                let (room, key) = keys.split_once('.').context("Invalid relay invitation")?;
                let endpoint = RelayEndpoint::parse(&format!("{scheme}{address}"))?;
                return Ok(Self {
                    destination: Destination::Relay {
                        endpoint,
                        room: decode_capability(room)?,
                    },
                    secret: decode_capability(key)?,
                });
            }
        }
        let body = text
            .trim()
            .strip_prefix("desklink://")
            .context("Paste the full desklink:// invitation from the host")?;
        let (address, secret) = body
            .split_once('#')
            .context("Invitation is missing its pairing key")?;
        let address: SocketAddr = address
            .parse()
            .context("Invitation must contain an IP address and port")?;
        ensure!(
            address.port() != 0 && !address.ip().is_unspecified() && !address.ip().is_multicast(),
            "Invitation has an invalid host address"
        );
        ensure!(secret.len() == 43, "Pairing key must be 256 bits");
        let mut key = [0; 32];
        let size = URL_SAFE_NO_PAD
            .decode_slice(secret, &mut key)
            .map_err(|_| anyhow::anyhow!("Invalid pairing key"))?;
        if size != 32 || key == [0; 32] {
            key.zeroize();
            bail!("Invalid pairing key");
        }
        Ok(Self {
            destination: Destination::Direct(address),
            secret: key,
        })
    }
}

impl fmt::Debug for Invitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Invitation")
            .field("address", &self.address_label())
            .field("secret", &"[REDACTED]")
            .finish()
    }
}
