use desklink::invitation::Invitation;

#[test]
fn pairing_invitation_round_trips_ipv4_and_ipv6() {
    for address in ["192.168.1.20:7070", "[::1]:7070"] {
        let invite = Invitation::new(address.parse().unwrap());
        let decoded = Invitation::parse(&invite.encode()).unwrap();
        assert_eq!(decoded.direct_address(), invite.direct_address());
        assert_eq!(decoded.secret(), invite.secret());
    }
}

#[test]
fn rejects_bad_invites_and_weak_or_oversized_keys() {
    for input in [
        "",
        "192.168.1.20:7070",
        "desklink://127.0.0.1:0#abc",
        "desklink://example.com:7070#abc",
        "desklink://127.0.0.1:7070#AA",
        "desklink://0.0.0.0:7070#abc",
    ] {
        assert!(Invitation::parse(input).is_err(), "accepted {input}");
    }
}

#[test]
fn every_hosting_session_has_a_new_secret_and_debug_hides_it() {
    let a = Invitation::new("127.0.0.1:7070".parse().unwrap());
    let b = Invitation::new(a.direct_address().unwrap());
    assert_ne!(a.secret(), b.secret());
    assert!(!format!("{a:?}").contains(a.encode().split('#').nth(1).unwrap()));
}

#[test]
fn relay_invitations_round_trip_without_revealing_capabilities_in_debug() {
    let endpoint = desklink::relay::RelayEndpoint::parse("wss://relay.example.com/v1").unwrap();
    let invite = Invitation::for_relay(endpoint, [19; 32]).unwrap();
    let encoded = invite.encode();
    let decoded = Invitation::parse(&encoded).unwrap();
    assert_eq!(decoded.encode(), encoded);
    assert_eq!(decoded.secret(), invite.secret());
    assert!(encoded.starts_with("desklink+relay://relay.example.com/v1#"));
    assert!(!format!("{invite:?}").contains(encoded.split('#').nth(1).unwrap()));
}

#[test]
fn relay_endpoints_reject_plaintext_nonlocal_and_ambiguous_urls() {
    use desklink::relay::RelayEndpoint;
    for endpoint in [
        "ws://relay.example.com/v1",
        "ws://192.168.1.2/v1",
        "wss://user:password@relay.example.com/v1",
        "wss://relay.example.com/v1?key=x",
        "wss://relay.example.com/v1#secret",
        "https://relay.example.com/v1",
        "wss://relay.example.com/other",
        "ws://localhost/v1",
    ] {
        assert!(
            RelayEndpoint::parse(endpoint).is_err(),
            "accepted {endpoint}"
        );
    }
    assert!(RelayEndpoint::parse("ws://127.0.0.1:9090/v1").is_ok());
    assert!(RelayEndpoint::parse("ws://[::1]:9090/v1").is_ok());
}
