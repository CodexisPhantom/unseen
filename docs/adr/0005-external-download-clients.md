# 0005 Keep download clients external

Status: accepted (2026-11-15)

## Context

Users already run qBittorrent, SABnzbd and Gluetun, with their own settings,
seeding rules and VPN setup. A built-in torrent client would be a large
project and would put the whole app behind the VPN.

## Decision

Unseen does not download. It drives qBittorrent (V1) and SABnzbd (M9)
through their APIs and reads the finished files from a shared volume. The
VPN container wraps only the torrent client.

## Consequences

- An existing download setup keeps working after the switch.
- Downloads and library must share a filesystem for hardlinks
  ([09-filesystem.md](../09-filesystem.md)).
- Download state lives in two places. Unseen reconciles by polling and after
  a restart ([06-state-machines.md](../06-state-machines.md)).
- Each supported client is an integration to maintain. V1 supports one.
