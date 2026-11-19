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

#[test]
fn frame_bounds_admit_sharp_and_original_but_reject_an_oversized_axis() {
    for (width, height, allowed) in [
        (2560, 1440, true),
        (1964, 3024, true),
        (4096, 4096, true),
        (4097, 1, false),
        (1, 4097, false),
        (u32::MAX, u32::MAX, false),
        (0, 1080, false),
    ] {
        let frame = Message::Frame {
            width,
            height,
            jpeg: vec![1],
        };
        assert_eq!(encode(&frame).is_ok(), allowed, "{width}x{height}");
    }
}

#[test]
fn profiles_define_the_default_and_quality_bounds() {
    use desklink::video::VideoProfile::{self, *};
    assert_eq!(VideoProfile::default(), Sharp);
    for (profile, edge, min, max) in [
        (Balanced, 1600, 40, 75),
        (Sharp, 2560, 55, 85),
        (Original, 4096, 65, 90),
    ] {
        assert_eq!(
            (
                profile.max_edge(),
                profile.min_quality(),
                profile.max_quality()
            ),
            (edge, min, max)
        );
        assert!(!profile.label().is_empty());
    }
}

#[test]
fn dimensions_preserve_aspect_with_valid_limits_and_large_inputs() {
    use desklink::video::scaled_dimensions;
    for (width, height, edge, expected) in [
        (3840, 2160, 2560, (2560, 1440)),
        (2160, 3840, 1600, (900, 1600)),
        (3024, 1964, 4096, (3024, 1964)),
        (3840, 2160, 1, (1, 1)),
        (u32::MAX, u32::MAX, 4096, (4096, 4096)),
        (1, u32::MAX, 2560, (1, 2560)),
    ] {
        assert_eq!(scaled_dimensions(width, height, edge).unwrap(), expected);
    }
    for (width, height, edge) in [(0, 1, 1600), (1, 0, 1600), (1, 1, 0), (1, 1, 4097)] {
        assert!(scaled_dimensions(width, height, edge).is_err());
    }
}

#[test]
fn video_settings_follow_all_legacy_discriminants_and_reject_unknown_profiles() {
    use desklink::video::VideoProfile;
    let messages = [
        Message::Request {
            name: "viewer".into(),
            control: false,
        },
        Message::Approved {
            desktop: DesktopInfo {
                name: "display".into(),
                width: 1,
                height: 1,
                x: 0,
                y: 0,
                scale: 1.0,
            },
            control: false,
        },
        Message::Denied(String::new()),
        Message::Frame {
            width: 1,
            height: 1,
            jpeg: vec![1],
        },
        Message::Input(InputEvent::ReleaseAll),
        Message::Ping(7),
        Message::Pong(7),
        Message::Error(String::new()),
        Message::Disconnect,
    ];
    for (index, message) in messages.iter().enumerate() {
        assert_eq!(encode(message).unwrap()[..4], (index as u32).to_le_bytes());
    }
    for (index, profile) in [
        VideoProfile::Balanced,
        VideoProfile::Sharp,
        VideoProfile::Original,
    ]
    .into_iter()
    .enumerate()
    {
        let bytes = encode(&Message::VideoSettings(profile)).unwrap();
        assert_eq!(bytes, [9, 0, 0, 0, index as u8, 0, 0, 0]);
        assert!(
            matches!(decode(&bytes).unwrap(), Message::VideoSettings(value) if value == profile)
        );
    }
    assert!(decode(&[9, 0, 0, 0, 3, 0, 0, 0]).is_err());
    assert!(decode(&[9, 0, 0, 0]).is_err());
}
