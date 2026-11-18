# Contributing

Contributions use the existing MIT license. Keep changes focused, explain their
user-visible behavior and include meaningful verification for capture, transport,
identity, permissions or input changes. Do not commit credentials, device identity
files, recordings, screenshots of private desktops, databases or signing material.

Run cargo fmt --all -- --check, cargo test --locked --no-default-features and the
relevant platform checks. Real capture/input tests require dedicated disposable
editors, explicit host approval and a guard that stops if the target loses focus.
Never send test input to unrelated applications or bypass OS security prompts.

The maintainer reviews external changes and approves releases. Signing requests
must use the traceable GitHub-hosted build and require manual release approval.
Maintainers with source or signing responsibilities must use MFA. See the
[Code signing policy](CODE_SIGNING.md).
