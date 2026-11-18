//! Public connection defaults. No enrollment or proxy credential is shipped.
pub const PUBLIC_RELAY_URL: &str = "wss://desklink.161.97.168.233.nip.io:9443/v2";

pub struct RelaySettings {
    pub url: String,
    pub access_code: String,
}

impl RelaySettings {
    pub fn from_overrides(url: Option<&str>, code: Option<&str>) -> Self {
        match url.map(str::trim).filter(|value| !value.is_empty()) {
            Some(url) => Self {
                url: url.into(),
                access_code: code.unwrap_or_default().into(),
            },
            // An unrelated private code must never be sent to the public default.
            None => Self {
                url: PUBLIC_RELAY_URL.into(),
                access_code: String::new(),
            },
        }
    }
}
