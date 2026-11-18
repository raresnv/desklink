use desklink::device::{ComputerId, Identity};
use std::path::PathBuf;
fn root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "desklink-identity-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ))
}
#[test]
fn identity_survives_restart_and_rejects_corruption() {
    let root = root();
    let a = Identity::load_or_create(&root).unwrap();
    let b = Identity::load_or_create(&root).unwrap();
    assert_eq!(a.public_key(), b.public_key());
    assert!(!format!("{a:?}").contains(&base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        a.private_key()
    )));
    std::fs::write(root.join("identity.key"), b"broken").unwrap();
    assert!(Identity::load_or_create(&root).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn computer_ids_accept_grouped_digits_but_not_ambiguous_addresses() {
    assert_eq!(
        ComputerId::parse("482 731 906 254").unwrap().to_string(),
        "482 731 906 254"
    );
    assert_eq!(
        ComputerId::parse("482-731-906-254").unwrap().as_str(),
        "482731906254"
    );
    for invalid in [
        "123",
        "000000000000",
        "1234567890123",
        "48273190625x",
        "127.0.0.1:7070",
    ] {
        assert!(ComputerId::parse(invalid).is_err());
    }
}
#[test]
fn approved_peer_keys_are_pinned_and_later_changes_fail_closed() {
    let root = root();
    let identity = Identity::load_or_create(&root).unwrap();
    let id = ComputerId::parse("482731906254").unwrap();
    identity
        .check_peer("wss://relay.example/v2", &id, &[7; 32])
        .unwrap();
    identity
        .remember_peer("wss://relay.example/v2", &id, &[7; 32])
        .unwrap();
    let restarted = Identity::load_or_create(&root).unwrap();
    restarted
        .check_peer("wss://relay.example/v2", &id, &[7; 32])
        .unwrap();
    assert!(restarted
        .check_peer("wss://relay.example/v2", &id, &[8; 32])
        .is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ownership_challenges_reject_impostors_and_replays() {
    use desklink::device::{answer_challenge, make_challenge, verify_answer};
    let root_a = root();
    let root_b = root();
    let a = Identity::load_or_create(&root_a).unwrap();
    let b = Identity::load_or_create(&root_b).unwrap();
    let (state, challenge) = make_challenge(a.public_key()).unwrap();
    assert!(answer_challenge(&b, &challenge).is_err());
    let answer = answer_challenge(&a, &challenge).unwrap();
    verify_answer(state, &answer).unwrap();
    let (other, _) = make_challenge(a.public_key()).unwrap();
    assert!(verify_answer(other, &answer).is_err());
    std::fs::remove_dir_all(root_a).unwrap();
    std::fs::remove_dir_all(root_b).unwrap();
}
#[test]
fn durable_directory_keeps_ids_and_enforces_capacity() {
    use desklink::device::registry::Directory;
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("directory.sqlite3");
    let directory = Directory::open(Some(&path), 1).unwrap();
    let id = directory.enroll(&[7; 32]).unwrap();
    assert_eq!(directory.enroll(&[7; 32]).unwrap(), id);
    assert!(directory.enroll(&[8; 32]).is_err());
    drop(directory);
    let directory = Directory::open(Some(&path), 1).unwrap();
    assert_eq!(directory.enroll(&[7; 32]).unwrap(), id);
    assert_eq!(directory.lookup(&id).unwrap(), Some([7; 32]));
    drop(directory);
    std::fs::remove_dir_all(root).unwrap();
}
