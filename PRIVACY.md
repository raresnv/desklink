# Privacy policy

This document describes the current DeskLink source and default public relay
behavior. Service operator: Overtex Core Solutions SRL.
Maintainer contact: rares@webleaf.ro.

## Local data

DeskLink creates a private device identity and stores it in the operating system's
per-user application-data directory under the DeskLink application name. It stores
relay-scoped computer IDs and remembered peer public keys in a local database.
The private identity is not sent to the relay. Deleting the local identity resets
the installation's identity and may require a new computer ID and renewed peer
trust. Removing only the app does not automatically delete these local records.

## Automatic relay connection

Opening the ordinary app automatically contacts
wss://desklink.161.97.168.233.nip.io:9443/v2. This obtains the computer's numeric ID
and makes approved pairing possible. Opening the app therefore discloses its
public device key, computer name and network source address to that service,
even before any screen-sharing request has been approved. The computer name can
initially derive from the operating system's COMPUTERNAME/HOSTNAME value.

The relay's device directory retains the numeric ID/public-key mapping. Online
state includes computer name, IP address, availability and pairing requests.
Registration rate-limit records include the source address and registration time;
the source prunes expired rate-limit records after approximately an hour.
The public relay creates daily backups and keeps its 30 most recent directory
snapshots, so metadata can remain in backups after live records expire. Database
WAL files and separate infrastructure backups may also retain older metadata;
source pruning is not a guarantee of immediate erasure from all backup copies.
The relay edge discards HTTP access logs for the connection service. Operational
container logs rotate by size (relay up to three 10 MB files; edge up to two 5 MB
files), rather than by a fixed number of days. Website download/access logging
is separate from the encrypted relay connection service.

The server can observe which devices request sessions and session traffic timing
and volume. Paired apps encrypt session screen/input data. The short-ID directory
is trusted on first contact; remembered peer-key changes are blocked. This does
not make the initial directory lookup independently trustworthy.

## Screen sharing and control

Screen frames are captured only for a host-approved session. A selected display
can contain personal or confidential information; the viewer can see everything
on that display. Host-approved control sends keyboard/mouse events and text
pasted through the viewer to the host. Viewing-only permission does not enable
remote input. Disconnect, revocation or app exit ends the session; focus changes
and the release shortcut release held input. The application implements no
built-in session recording, analytics or advertising subsystem. Operating-system
services and users' separate recording tools are outside that statement.

## User choices

Closing the app stops its automatic relay connection and any session. Turning
off Accept access requests stops incoming access requests, but does not promise
to disconnect presence from the relay. Advanced connection settings support a
custom relay; that operator has its own network/log/backup practices. The user
can change the computer name shown to peers and must approve every sharing
session. OS capture/control permissions can be revoked through OS settings.

For questions about the operated public relay, retention or deletion requests,
contact the maintainer/service operator above. Deletion of a device directory
entry can invalidate its numeric ID; requests must be verified to avoid deleting
another user’s device. Backup copies expire through the backup rotation process
rather than selective immediate deletion. Self-hosted operators are responsible
for their own service policies.
