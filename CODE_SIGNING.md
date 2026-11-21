# Code signing policy

## Current status

The SignPath Foundation application was not approved on 19 November 2026.
DeskLink has no Foundation certificate or active Foundation signing relationship.
Windows installers, app executables and uninstallers remain unsigned previews. No endorsement or code-signing service is
implied. No paid signing subscription has been selected as a replacement.

Mac signing is separate: the Mac 0.3.3 preview is Developer ID signed by
Overtex Core Solutions SRL, Apple notarized and includes its stapled ticket.
The default Mac download is a separately signed/notarized DMG with its own
stapled ticket. See [the DMG release record](docs/MAC-DMG-0.3.3.md).
Windows publisher signing is not configured. See each public download's actual
version and signing status.

## Project roles

Initial project roles:

- Author/committer: Rares Nastai, [raresnv](https://github.com/raresnv).
- Reviewer of external changes: Rares Nastai, [raresnv](https://github.com/raresnv).
- Release/signing approver: Rares Nastai, [raresnv](https://github.com/raresnv).

The same initial maintainer holds these roles. This does not claim independent
review or a multi-person approval threshold. All maintainers must enable MFA on
GitHub. Any future signing-service accounts must also use MFA. Additional
authors/reviewers/approvers must be recorded here
before being granted signing responsibilities.

## Release controls

Any future Windows signing will accept only project executables produced from
this public source by GitHub-hosted Actions, uploaded as workflow artifacts and verified by
the configured provider's trusted build integration. Local test executables and
files supplied from outside that build will not be substituted for signed artifacts.

Every signed release needs manual approval in the configured signing service.
Pull requests and forks must not receive signing credentials. Missing configuration, rejection or invalid
Authenticode trust/timestamp must stop signed packaging/publication. The signed
artifact's expected publisher, product/version and archive/executable hashes
must be verified before new immutable download links are published.

## Privacy

See the [Privacy policy](PRIVACY.md). The app automatically connects to the
configured public relay on launch; its metadata handling is described there.
No screen capture or remote input session starts without explicit host approval.
