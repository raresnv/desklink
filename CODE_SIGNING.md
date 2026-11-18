# Code signing policy

## Current status

DeskLink is preparing an application to SignPath Foundation for Windows signing.
The Foundation has not accepted the project and has not signed its releases.
Existing public Windows preview downloads remain unsigned. No endorsement or
active signing relationship is implied by this application.

If the project is accepted, the Windows signing attribution will be:

> Free code signing provided by [SignPath.io](https://signpath.io/), certificate
> by [SignPath Foundation](https://signpath.org/).

A Foundation signature will identify SignPath Foundation as publisher rather
than the maintainer's personal name. Mac releases use their separate Developer
ID Application identity, Overtex Core Solutions SRL, and Apple notarization.

## Project roles

Initial project roles:

- Author/committer: Rares Nastai, [raresnv](https://github.com/raresnv).
- Reviewer of external changes: Rares Nastai, [raresnv](https://github.com/raresnv).
- Release/signing approver: Rares Nastai, [raresnv](https://github.com/raresnv).

The same initial maintainer holds these roles. This does not claim independent
review or a multi-person approval threshold. All maintainers must enable MFA on
GitHub and SignPath. Additional authors/reviewers/approvers must be recorded here
before being granted signing responsibilities.

## Release controls

Windows signing will accept only project executables produced from this public
source by GitHub-hosted Actions, uploaded as workflow artifacts and verified by
SignPath's trusted build integration. Local test executables and files supplied
from outside that build will not be substituted for signed artifacts.

Every release needs manual approval in SignPath. Pull requests and forks must
not receive signing credentials. Missing configuration, rejection or invalid
Authenticode trust/timestamp must stop signed packaging/publication. The signed
artifact's expected publisher, product/version and archive/executable hashes
must be verified before new immutable download links are published.

## Privacy

See the [Privacy policy](PRIVACY.md). The app automatically connects to the
configured public relay on launch; its metadata handling is described there.
No screen capture or remote input session starts without explicit host approval.
