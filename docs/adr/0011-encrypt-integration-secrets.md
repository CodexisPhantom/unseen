# 0011 Encrypt integration secrets at rest

Status: accepted (2026-11-15)

## Context

Unseen stores secrets it must be able to use: indexer API keys, download
client passwords, the TMDB key, proxy credentials, import source keys.
Sonarr and Radarr store these in plain text. A copied database file or
backup then reveals all of them.

## Decision

- Integration secrets are stored encrypted in SQLite.
- The key is derived with HKDF from `UNSEEN_SECRET`, which lives outside the
  database (environment variable or a file in the data directory).
- Cipher: XChaCha20-Poly1305 with a random nonce per value. Stored form:
  `v1:<nonce>:<ciphertext>`.
- One helper in `crates/db` encrypts and decrypts. Entities expose a secret
  type that does not implement `Debug` or `Serialize` with its content.
- Passwords and Unseen API keys are not affected: they are hashed, not
  encrypted.

## Consequences

- A stolen database file alone does not reveal integration secrets. A
  backup that contains both the database and the secret does.
- Losing `UNSEEN_SECRET` means re-entering every integration secret. The
  app starts and marks those entries.
- The secret must be part of the documented backup.
- No protection against someone who can read both the data directory and
  the database, or against a compromised host.
