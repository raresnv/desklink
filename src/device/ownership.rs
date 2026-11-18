use super::Identity;
use anyhow::{ensure, Result};
use snow::{Builder, HandshakeState};
const PATTERN: &str = "Noise_NK_25519_ChaChaPoly_BLAKE2s";
const CONTEXT: &[u8] = b"DeskLink-device-ownership-v2";
const PROOF: &[u8] = b"DeskLink-device-proof-v2";
/// Each challenge uses a new Noise ephemeral key; answers are connection-bound.
pub fn make_challenge(public: &[u8; 32]) -> Result<(HandshakeState, Vec<u8>)> {
    let mut state = Builder::new(PATTERN.parse()?)
        .prologue(CONTEXT)
        .remote_public_key(public)
        .build_initiator()?;
    let mut bytes = vec![0; 512];
    let size = state.write_message(&[], &mut bytes)?;
    bytes.truncate(size);
    Ok((state, bytes))
}
pub fn answer_challenge(identity: &Identity, challenge: &[u8]) -> Result<Vec<u8>> {
    ensure!(challenge.len() <= 512, "Ownership challenge too large");
    let mut state = Builder::new(PATTERN.parse()?)
        .prologue(CONTEXT)
        .local_private_key(identity.private_key())
        .build_responder()?;
    let mut bytes = vec![0; 512];
    state.read_message(challenge, &mut bytes)?;
    let size = state.write_message(PROOF, &mut bytes)?;
    bytes.truncate(size);
    ensure!(state.is_handshake_finished(), "Incomplete identity proof");
    Ok(bytes)
}
pub fn verify_answer(mut state: HandshakeState, answer: &[u8]) -> Result<()> {
    ensure!(answer.len() <= 512, "Ownership proof too large");
    let mut bytes = [0; 512];
    let size = state.read_message(answer, &mut bytes)?;
    ensure!(
        state.is_handshake_finished() && &bytes[..size] == PROOF,
        "Invalid ownership proof"
    );
    Ok(())
}
