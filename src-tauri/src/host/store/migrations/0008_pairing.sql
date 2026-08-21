-- Devices this host has been paired with (#19).
--
-- The durable half of the handshake in `host/pairing/`. The offer itself — the
-- QR, the secret, the safety number being compared right now — is deliberately
-- RAM-only and explained there; what has to survive a quit is the *outcome*:
-- which devices were admitted, what each may do, and which have been revoked.
--
-- Three columns carry the security properties this table exists for.
--
-- `role` is the scope. It is written here by the human on the *host* at
-- confirmation time and read fresh on every subsequent call, so it can never
-- be asserted by the client that is being scoped, and narrowing a device takes
-- effect on its next request rather than at its next reconnect.
--
-- `revoked_at` is a tombstone rather than a DELETE. `pairing-security-mobile.md`
-- sketches revoke as "deletes the row"; a tombstone keeps the promise (the
-- device is refused from the moment the row is stamped, and the stamp is on
-- disk before the answer goes out) and additionally answers "was this phone
-- ever paired, and when did we cut it off" — the question a stolen phone
-- actually raises. `device_id` therefore stays a primary key: a revoked device
-- re-pairs by re-running the handshake, which un-revokes its row with fresh
-- key material.
--
-- `token_ref` is a pointer into the secrets vault, never the secret. Same rule
-- the whole store follows (`store/secrets.rs`): SQLite holds references, the
-- OS keychain holds bytes. `auth_counter` is the replay guard beside it — a
-- `host/hello` proof must carry a counter strictly greater than the last one
-- accepted, so a captured proof cannot be replayed even on a wire with no
-- confidentiality.
--
-- `sas` is kept for the human, not the protocol: it is the safety number both
-- ends compared when this device was admitted, so a device list can say what
-- was verified and a host-key change can be explained rather than just felt.

CREATE TABLE paired_devices (
  device_id     TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  role          TEXT NOT NULL CHECK (role IN ('full', 'approver')),
  -- The device's public commitment to its own long-term key material. Both
  -- fingerprints are folded into the safety number, which is what makes the
  -- number depend on both sides rather than only on the host.
  fingerprint   TEXT NOT NULL,
  -- Vault account holding the shared device token. Not the token.
  token_ref     TEXT NOT NULL,
  auth_counter  INTEGER NOT NULL DEFAULT 0,
  paired_via    TEXT NOT NULL CHECK (paired_via IN ('qr', 'code')),
  sas           TEXT NOT NULL,
  created_at    TEXT NOT NULL,
  last_seen_at  TEXT,
  revoked_at    TEXT
);

CREATE INDEX paired_devices_live ON paired_devices(revoked_at);
