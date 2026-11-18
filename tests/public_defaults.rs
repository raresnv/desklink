use desklink::{config::RelaySettings, relay::RelayEndpoint};

#[test]
fn ordinary_launch_uses_public_tls_relay_without_a_shared_secret() {
    let settings = RelaySettings::from_overrides(None, None);
    assert_eq!(settings.url, "wss://desklink.161.97.168.233.nip.io:9443/v2");
    assert!(settings.access_code.is_empty());
    RelayEndpoint::parse(&settings.url).unwrap();
}

#[test]
fn private_override_is_preserved_and_stray_secret_never_goes_to_public_default() {
    let private =
        RelaySettings::from_overrides(Some("ws://127.0.0.1:8080/v2"), Some("private-test-code"));
    assert_eq!(private.url, "ws://127.0.0.1:8080/v2");
    assert_eq!(private.access_code, "private-test-code");
    assert!(
        RelaySettings::from_overrides(None, Some("private-test-code"))
            .access_code
            .is_empty()
    );
    assert!(
        RelaySettings::from_overrides(Some(" "), Some("private-test-code"))
            .access_code
            .is_empty()
    );
}

#[test]
fn public_tls_listener_keeps_existing_identity_and_peer_trust_realm() {
    let original = RelayEndpoint::parse("wss://desklink.161.97.168.233.nip.io/v2").unwrap();
    let current = RelayEndpoint::parse("wss://desklink.161.97.168.233.nip.io:9443/v2").unwrap();
    assert_eq!(original.identity_realm(), current.identity_realm());
    assert_eq!(
        current.device_url(),
        "wss://desklink.161.97.168.233.nip.io:9443/v2"
    );
    for host in [
        "relay.example.com",
        "desklink.161.97.168.233.nip.io.evil.example",
    ] {
        let a = RelayEndpoint::parse(&format!("wss://{host}/v2")).unwrap();
        let b = RelayEndpoint::parse(&format!("wss://{host}:9443/v2")).unwrap();
        assert_ne!(a.identity_realm(), b.identity_realm());
    }
    let unrelated_port =
        RelayEndpoint::parse("wss://desklink.161.97.168.233.nip.io:8443/v2").unwrap();
    assert_ne!(original.identity_realm(), unrelated_port.identity_realm());
}
