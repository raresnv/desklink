use desklink::protocol::{decode, encode, AccessPolicy, DesktopInfo, InputEvent, Message};

#[test]
fn consent_and_control_are_independent_requirements() {
    let mut policy = AccessPolicy::default();
    assert!(!policy.can_view());
    assert!(!policy.can_control());
    policy.approve(false);
    assert!(policy.can_view());
    assert!(!policy.can_control());
    policy.approve(true);
    assert!(policy.can_control());
    policy.revoke();
    assert!(!policy.can_view());
    assert!(!policy.can_control());
}

#[test]
fn pointer_maps_to_monitor_with_negative_origin_and_clamps_edges() {
    let info = DesktopInfo {
        name: "Left".into(),
        width: 1920,
        height: 1080,
        x: -1920,
        y: 0,
        scale: 1.0,
    };
    assert_eq!(info.map_pointer(0.0, 0.0).unwrap(), (-1920, 0));
    assert_eq!(info.map_pointer(1.0, 1.0).unwrap(), (-1, 1079));
    assert_eq!(info.map_pointer(-1.0, 2.0).unwrap(), (-1920, 1079));
    assert!(info.map_pointer(f32::NAN, 0.0).is_err());
}

#[test]
fn retina_coordinates_use_logical_points() {
    let info = DesktopInfo {
        name: "Retina".into(),
        width: 3024,
        height: 1964,
        x: 0,
        y: 0,
        scale: 2.0,
    };
    assert_eq!(info.map_pointer(1.0, 1.0).unwrap(), (1511, 981));
}

#[test]
fn bounded_messages_reject_trailing_data_and_unreasonable_input() {
    let message = Message::Input(InputEvent::Text("hello".into()));
    let mut bytes = encode(&message).unwrap();
    assert!(matches!(decode(&bytes).unwrap(), Message::Input(_)));
    bytes.push(0);
    assert!(decode(&bytes).is_err());
    assert!(encode(&Message::Input(InputEvent::Text("x".repeat(8193)))).is_err());
}
