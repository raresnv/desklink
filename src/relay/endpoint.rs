use anyhow::{ensure, Context, Result};
use std::net::IpAddr;
use url::{Host, Url};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayEndpoint(String);

impl RelayEndpoint {
    pub fn parse(text: &str) -> Result<Self> {
        ensure!(text.len() <= 240, "Relay address is too long");
        let url = Url::parse(text.trim())
            .context("Enter a relay address such as wss://relay.example.com/v1")?;
        let local = match url.host() {
            Some(Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
            Some(Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
            _ => false,
        };
        ensure!(url.scheme() == "wss" || (url.scheme() == "ws" && local), "Internet relays require wss:// (TLS); ws:// is only allowed on numeric loopback addresses");
        ensure!(
            url.host().is_some() && url.port() != Some(0),
            "Invalid relay host or port"
        );
        ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && matches!(url.path(), "/v1" | "/v2"),
            "Relay address must end in /v1 or /v2, without credentials, query, or fragment"
        );
        Ok(Self(url.into()))
    }

    pub fn device_url(&self) -> String {
        let mut url = Url::parse(&self.0).expect("validated relay endpoint");
        url.set_path("/v2");
        url.into()
    }

    /// The two operated public listeners share one directory and existing pins.
    /// Other hosts and ports remain distinct trust realms.
    pub fn identity_realm(&self) -> String {
        let mut url = Url::parse(&self.device_url()).expect("validated endpoint");
        if url.scheme() == "wss"
            && url.host_str() == Some("desklink.161.97.168.233.nip.io")
            && url.port() == Some(9443)
        {
            url.set_port(None).expect("validated public port");
        }
        url.into()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
