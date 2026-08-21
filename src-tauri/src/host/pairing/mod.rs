//! Pairing a second device (#19): QR, safety number, scoped grant, revoke.
//!
//! `docs/research/remote-and-mobile/pairing-security-mobile.md` is the source.
//! Its flow, in the order this module implements it: the host generates a
//! single-use, short-lived offer; the QR carries the host's identity and that
//! secret; the new device proves possession and presents key material of its
//! own; **both screens show the same safety number** and both humans confirm;
//! the host records `{ deviceId, name, role, … }`; revoke is a list on the
//! host.
//!
//! ## What the handshake actually guarantees
//!
//! Being precise here matters more than the happy path, because the honest
//! answer is narrower than "end-to-end encrypted pairing".
//!
//! There is **no asymmetric cryptography in this host.** The dependency set is
//! deliberately tiny (`src-tauri/Cargo.toml`), and a hand-rolled curve is a
//! worse idea than an honest symmetric handshake. So:
//!
//! - A "fingerprint" is a **commitment** to key material — `H(domain, key)` —
//!   not a verifying key. It lets each side name its long-term key without
//!   revealing it, and it makes a reinstall visible (the fingerprint changes).
//!   It does **not** let either side verify a signature.
//! - Authentication comes from the **out-of-band channel**: the secret in the
//!   QR, or the code read aloud. Both sides MAC the transcript with it, so a
//!   man in the middle who never saw the screen cannot produce either proof
//!   and the handshake stops at the MAC check — before any safety number is
//!   shown.
//! - The **safety number is derived from both sides' material** — both
//!   fingerprints, both nonces, the pairing id and which channel was used —
//!   keyed by the out-of-band secret. It is the check that survives an
//!   attacker who *did* see the secret (a shoulder-surfed QR, a code typed in
//!   a café) and tried to interpose their own device: their transcript is a
//!   different transcript, so the two humans see different numbers. A number
//!   only one side computes would prove nothing, which is why
//!   `pairing/claim` deliberately does **not** return it and why
//!   `pairing/confirm` makes both sides state the number they derived.
//! - The pairing yields a shared **device token** that is *derived on both
//!   sides and never transmitted*. The host keeps it in the OS keychain, not
//!   in SQLite.
//!
//! What this does not do: it does not encrypt the transport (there is no
//! remote transport yet — decision #4 keeps the host in-process until a second
//! client exists), and it does not let a device authenticate the host on later
//! connections. Both are the transport's job when one exists, and rule 1 of
//! the research says that transport must be TLS or Noise. Nothing here should
//! be read as a claim that the wire is safe on its own.
//!
//! ## Where the state lives
//!
//! Offers are RAM (see [`offer`]); grants are SQLite plus the vault. That
//! split is the security property: a photographed QR is worthless after a
//! restart, while a revoke survives one.
//!
//! ## Scope
//!
//! A grant is a role, not a key to everything — see [`scope`]. It is checked
//! on **every** request, against the row rather than a cached value, so a
//! revoke takes effect on a device's next call rather than its next connect.

pub(crate) mod crypto;
mod offer;
pub(crate) mod scope;

use std::collections::HashMap;

use chrono::Utc;
use uuid::Uuid;

use super::protocol::error::RpcError;
use super::protocol::methods::{
    DeviceAuth, DeviceInfo, DeviceListResult, DeviceRefParams, DeviceRevokeResult, DeviceRole,
    PairedDeviceView, PairingCancelResult, PairingClaimParams, PairingClaimResult,
    PairingConfirmParams, PairingConfirmResult, PairingDevice, PairingOfferView, PairingQr,
    PairingRefParams, PairingSide, PairingStartParams, PairingStartResult, PairingStatusResult,
    PROTOCOL_VERSION,
};
use super::store::{secret_account, NewPairedDevice};
use super::tools::crypto::{base64url, random_token};
use super::HostSession;
use crypto::{crockford, ct_eq, hex, hmac_sha256, sas_digits, transcript_hash};
use offer::{Channel, Claim, Offer, OfferState};

/// Domain separators. Every derivation is keyed by the out-of-band credential
/// and separated by one of these, so a proof made for one step is not a valid
/// proof for another.
const TRANSCRIPT_DOMAIN: &str = "jabot/pairing/v1";
const CLAIM_DOMAIN: &str = "jabot/pairing/claim/v1";
const HOST_DOMAIN: &str = "jabot/pairing/host/v1";
const CONFIRM_DOMAIN: &str = "jabot/pairing/confirm/v1";
const SAS_DOMAIN: &str = "jabot/pairing/sas/v1";
const TOKEN_DOMAIN: &str = "jabot/pairing/device-token/v1";
const HELLO_DOMAIN: &str = "jabot/hello/v1";

const QR_VERSION: u32 = 1;
const CODE_LEN: usize = 8;

/// Live offers. Everything durable about pairing is a row; this is the part
/// that is *meant* to die with the process.
#[derive(Debug, Default)]
pub(crate) struct PairingState {
    offers: HashMap<String, Offer>,
}

/// The full transcript, hex-encoded — the input every derivation is bound to.
///
/// Order and framing are part of the contract; see [`PairingClaimParams`].
fn transcript(
    host_id: &str,
    host_fingerprint: &str,
    host_nonce: &str,
    pairing_id: &str,
    device: &PairingDevice,
    via: Channel,
) -> String {
    hex(&transcript_hash(&[
        TRANSCRIPT_DOMAIN,
        host_id,
        host_fingerprint,
        host_nonce,
        pairing_id,
        &device.device_id,
        &device.fingerprint,
        &device.nonce,
        via.as_str(),
    ]))
}

/// One derivation from the out-of-band credential and the transcript.
fn bind(key: &str, domain: &str, transcript_hex: &str) -> [u8; 32] {
    hmac_sha256(key.as_bytes(), &transcript_hash(&[domain, transcript_hex]))
}

fn failed(reason: &'static str, detail: impl Into<String>) -> RpcError {
    RpcError::PairingFailed {
        reason,
        detail: detail.into(),
    }
}

/// Where a device's shared token lives in the vault. The row stores this
/// string; the bytes never touch SQLite.
fn token_account(device_id: &str) -> String {
    secret_account(&format!("device-token.{device_id}"))
}

impl HostSession {
    // ---- host side ------------------------------------------------------

    /// Put a QR on the screen. `full` devices only (enforced by [`scope`]).
    pub fn pairing_start(
        &mut self,
        params: PairingStartParams,
    ) -> Result<PairingStartResult, RpcError> {
        // Refused rather than offered on a host with no store: the handshake
        // would run and then have nowhere to record the grant, which is a
        // worse outcome than saying no before the user walks to their phone.
        if self.store.is_none() {
            return Err(RpcError::StoreUnavailable);
        }
        self.prune_offers();
        if self.pairing.offers.len() >= offer::MAX_OPEN_OFFERS {
            return Err(failed(
                "too_many",
                "too many pairing offers are already open",
            ));
        }

        let now = Utc::now();
        let ttl = offer::ttl_seconds(params.ttl_secs);
        let entry = Offer {
            id: Uuid::new_v4().to_string(),
            secret: random_token(),
            code: crockford(Uuid::new_v4().as_bytes(), CODE_LEN),
            host_nonce: random_token(),
            expires_at: offer::expiry(now, ttl),
            attempts: 0,
            state: OfferState::Offered,
        };

        let qr = PairingQr {
            v: QR_VERSION,
            host_id: self.identity.host_id.clone(),
            host_name: self.identity.host_name.clone(),
            host_fingerprint: self.identity.host_fingerprint(),
            pairing_id: entry.id.clone(),
            host_nonce: entry.host_nonce.clone(),
            secret: entry.secret.clone(),
            // No reachable address to publish while the only client is
            // colocated. A phone learns where to connect from whatever
            // transport ships with #4's extraction, not from a guess made here.
            addrs: Vec::new(),
        };
        let qr_payload =
            serde_json::to_string(&qr).map_err(|e| RpcError::Internal(e.to_string()))?;

        let result = PairingStartResult {
            pairing_id: entry.id.clone(),
            host_id: qr.host_id.clone(),
            host_name: qr.host_name.clone(),
            host_fingerprint: qr.host_fingerprint.clone(),
            host_nonce: entry.host_nonce.clone(),
            secret: entry.secret.clone(),
            code: entry.code.clone(),
            expires_at: entry.expires_at.to_rfc3339(),
            qr_payload,
        };
        self.pairing.offers.insert(entry.id.clone(), entry);
        Ok(result)
    }

    /// Live offers, without their credentials.
    pub fn pairing_status(&mut self) -> Result<PairingStatusResult, RpcError> {
        self.prune_offers();
        let mut offers: Vec<PairingOfferView> = self
            .pairing
            .offers
            .values()
            .map(|entry| {
                let claim = entry.claim();
                PairingOfferView {
                    pairing_id: entry.id.clone(),
                    state: entry.state_name().to_string(),
                    expires_at: entry.expires_at.to_rfc3339(),
                    attempts: entry.attempts,
                    host_confirmed: claim.is_some_and(|c| c.host_confirmed),
                    device_confirmed: claim.is_some_and(|c| c.device_confirmed),
                    sas: claim.map(|c| c.sas.clone()),
                    device: claim.map(|c| PairingDevice {
                        device_id: c.device_id.clone(),
                        name: c.display_name(),
                        fingerprint: c.device_fingerprint.clone(),
                        nonce: c.device_nonce.clone(),
                    }),
                    via: claim.map(|c| c.via.as_str().to_string()),
                }
            })
            .collect();
        offers.sort_by(|a, b| a.expires_at.cmp(&b.expires_at));
        Ok(PairingStatusResult { offers })
    }

    /// Drop an offer. Idempotent: an id nobody has heard of is already gone.
    pub fn pairing_cancel(
        &mut self,
        params: PairingRefParams,
    ) -> Result<PairingCancelResult, RpcError> {
        let cancelled = self.pairing.offers.remove(&params.pairing_id).is_some();
        Ok(PairingCancelResult {
            pairing_id: params.pairing_id,
            cancelled,
        })
    }

    // ---- device side ----------------------------------------------------

    /// Claim an offer with the out-of-band credential and a proof.
    ///
    /// Answered before any `host/hello`, because the device claiming it is by
    /// definition not paired yet — that is what this call is for. It is safe
    /// to answer unauthenticated for exactly one reason: without the secret
    /// from the host's own screen there is no way past the MAC check, and
    /// three wrong tries burn the offer.
    pub fn pairing_claim(
        &mut self,
        params: PairingClaimParams,
    ) -> Result<PairingClaimResult, RpcError> {
        if self.store.is_none() {
            return Err(RpcError::StoreUnavailable);
        }
        validate_device(&params.device)?;
        self.prune_offers();

        let host_id = self.identity.host_id.clone();
        let host_name = self.identity.host_name.clone();
        let host_fingerprint = self.identity.host_fingerprint();

        // Read everything the derivations need in one pass, so the rest of the
        // method is arithmetic rather than a borrow held across it.
        let (host_nonce, channel, key, existing, expires_at) = {
            let entry = self
                .pairing
                .offers
                .get(&params.pairing_id)
                .ok_or_else(|| failed("unknown", "no such pairing offer"))?;
            let channel = entry.match_channel(params.secret.as_deref(), params.code.as_deref());
            (
                entry.host_nonce.clone(),
                channel,
                channel.map(|via| entry.channel_key(via).to_string()),
                entry.claim().cloned(),
                entry.expires_at.to_rfc3339(),
            )
        };

        let (Some(via), Some(key)) = (channel, key) else {
            // A wrong credential costs the offer one of its three lives. That
            // is what makes the eight-character code defensible: its entropy
            // is a human's, and the offer's patience is not.
            return Err(self.burn_attempt(
                &params.pairing_id,
                "credential",
                "wrong pairing credential",
            ));
        };

        let transcript_hex = transcript(
            &host_id,
            &host_fingerprint,
            &host_nonce,
            &params.pairing_id,
            &params.device,
            via,
        );

        // A second claim by the *same* device with the *same* material is a
        // retry — a dropped response, a reconnect — and re-answering it costs
        // nothing, because the answer is a pure function of the transcript. A
        // claim by anyone else is the single-use rule doing its job.
        if let Some(existing) = existing.as_ref() {
            let same = existing.device_id == params.device.device_id
                && existing.device_fingerprint == params.device.fingerprint
                && existing.device_nonce == params.device.nonce
                && existing.via == via;
            if !same {
                return Err(failed(
                    "claimed",
                    "this pairing offer has already been used",
                ));
            }
        }

        if !ct_eq(
            params.mac.as_bytes(),
            hex(&bind(&key, CLAIM_DOMAIN, &transcript_hex)).as_bytes(),
        ) {
            return Err(self.burn_attempt(
                &params.pairing_id,
                "proof",
                "the device's pairing proof did not verify",
            ));
        }

        let sas = sas_digits(&bind(&key, SAS_DOMAIN, &transcript_hex));
        let host_mac = hex(&bind(&key, HOST_DOMAIN, &transcript_hex));
        let device_token = base64url(&bind(&key, TOKEN_DOMAIN, &transcript_hex));

        let state = {
            let entry = self
                .pairing
                .offers
                .get_mut(&params.pairing_id)
                .ok_or_else(|| failed("unknown", "no such pairing offer"))?;
            if entry.claim().is_none() {
                entry.state = OfferState::Claimed(Box::new(Claim {
                    device_id: params.device.device_id.clone(),
                    device_name: params.device.name.clone(),
                    device_fingerprint: params.device.fingerprint.clone(),
                    device_nonce: params.device.nonce.clone(),
                    via,
                    sas,
                    device_token,
                    host_confirmed: false,
                    device_confirmed: false,
                    role: None,
                    name_override: None,
                }));
            }
            entry.state_name().to_string()
        };

        Ok(PairingClaimResult {
            pairing_id: params.pairing_id,
            host_id,
            host_name,
            host_fingerprint,
            host_nonce,
            host_mac,
            via: via.as_str().to_string(),
            expires_at,
            state,
        })
    }

    /// One wrong guess: spend a life, and close the offer when they run out.
    ///
    /// Every failed presentation goes through here, so "wrong secret" and
    /// "wrong proof" cost the same and neither can be probed for free.
    fn burn_attempt(&mut self, pairing_id: &str, reason: &'static str, detail: &str) -> RpcError {
        let spent = match self.pairing.offers.get_mut(pairing_id) {
            Some(entry) => {
                entry.attempts += 1;
                entry.is_spent()
            }
            None => false,
        };
        if spent {
            self.pairing.offers.remove(pairing_id);
        }
        failed(
            reason,
            if spent {
                format!("{detail}; this offer is now closed")
            } else {
                detail.to_string()
            },
        )
    }

    // ---- both sides -----------------------------------------------------

    /// "The number on my screen is this one." Both sides must say it.
    pub fn pairing_confirm(
        &mut self,
        params: PairingConfirmParams,
    ) -> Result<PairingConfirmResult, RpcError> {
        // The host side is an administrative action on the machine that can
        // run a shell, so it needs a connection that has said hello — and
        // [`scope`] has already refused it for anything but a `full` device.
        if params.side == PairingSide::Host {
            self.require_hello()?;
        }
        self.prune_offers();

        let host_id = self.identity.host_id.clone();
        let host_fingerprint = self.identity.host_fingerprint();

        {
            let entry = self
                .pairing
                .offers
                .get_mut(&params.pairing_id)
                .ok_or_else(|| failed("unknown", "no such pairing offer"))?;
            let host_nonce = entry.host_nonce.clone();
            let offer_id = entry.id.clone();
            let key = {
                let claim = entry
                    .claim()
                    .ok_or_else(|| failed("unclaimed", "nobody has scanned this offer yet"))?;
                entry.channel_key(claim.via).to_string()
            };

            let claim = entry
                .claim_mut()
                .ok_or_else(|| failed("unclaimed", "nobody has scanned this offer yet"))?;

            // Both sides state the number they derived, and it has to be the
            // one the host derived too. This is the check a man in the middle
            // who substituted key material cannot pass: their transcript is a
            // different transcript, so their safety number is a different
            // number, and the pairing stops here rather than merely looking
            // odd to whoever was paying attention.
            if !ct_eq(params.sas.as_bytes(), claim.sas.as_bytes()) {
                return Err(failed(
                    "sas",
                    "the safety numbers do not match; do not complete this pairing",
                ));
            }

            match params.side {
                PairingSide::Host => {
                    claim.host_confirmed = true;
                    claim.role = Some(params.role.unwrap_or(DeviceRole::Approver));
                    claim.name_override = params
                        .name
                        .as_deref()
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(str::to_string);
                }
                PairingSide::Device => {
                    let device = PairingDevice {
                        device_id: claim.device_id.clone(),
                        name: claim.device_name.clone(),
                        fingerprint: claim.device_fingerprint.clone(),
                        nonce: claim.device_nonce.clone(),
                    };
                    let transcript_hex = transcript(
                        &host_id,
                        &host_fingerprint,
                        &host_nonce,
                        &offer_id,
                        &device,
                        claim.via,
                    );
                    let expected = hex(&bind(&key, CONFIRM_DOMAIN, &transcript_hex));
                    let mac = params.mac.as_deref().unwrap_or_default();
                    if !ct_eq(mac.as_bytes(), expected.as_bytes()) {
                        return Err(failed(
                            "proof",
                            "the device's confirmation proof did not verify",
                        ));
                    }
                    claim.device_confirmed = true;
                }
            }
        }

        let both = self
            .pairing
            .offers
            .get(&params.pairing_id)
            .and_then(Offer::claim)
            .is_some_and(Claim::both_confirmed);
        if !both {
            let state = self
                .pairing
                .offers
                .get(&params.pairing_id)
                .map(|entry| entry.state_name().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Ok(PairingConfirmResult {
                pairing_id: params.pairing_id,
                state,
                device: None,
            });
        }

        let device = self.complete_pairing(&params.pairing_id)?;
        Ok(PairingConfirmResult {
            pairing_id: params.pairing_id,
            state: "paired".to_string(),
            device: Some(device),
        })
    }

    /// Write the grant: token to the vault, row to SQLite, offer burned.
    ///
    /// Vault first. A row that names a token the vault does not have is a
    /// device that can never authenticate and cannot tell why; a vault entry
    /// with no row is unreachable garbage, and it is cleaned up below.
    fn complete_pairing(&mut self, pairing_id: &str) -> Result<DeviceInfo, RpcError> {
        let Some(entry) = self.pairing.offers.remove(pairing_id) else {
            return Err(failed("unknown", "no such pairing offer"));
        };
        let Some(claim) = entry.claim().cloned() else {
            return Err(failed("unclaimed", "nobody has scanned this offer yet"));
        };

        let account = token_account(&claim.device_id);
        if let Err(err) = self.secrets.put(&account, &claim.device_token) {
            // Put the offer back so the two humans can simply press the button
            // again once the keychain is reachable.
            self.pairing.offers.insert(entry.id.clone(), entry);
            return Err(failed(
                "vault",
                format!("could not store the device's key material: {err}"),
            ));
        }

        let role = claim.granted_role();
        let new = NewPairedDevice {
            device_id: claim.device_id.clone(),
            name: claim.display_name(),
            role: role.as_str().to_string(),
            fingerprint: claim.device_fingerprint.clone(),
            token_ref: account.clone(),
            paired_via: claim.via.as_str().to_string(),
            sas: claim.sas.clone(),
        };
        let store = self.store.as_ref().ok_or(RpcError::StoreUnavailable)?;
        match store.upsert_paired_device(&new) {
            Ok(row) => Ok(DeviceInfo {
                device_id: row.device_id,
                name: row.name,
                role,
                created_at: Some(row.created_at),
            }),
            Err(err) => {
                let _ = self.secrets.delete(&account);
                Err(RpcError::Internal(format!(
                    "could not record the paired device: {err}"
                )))
            }
        }
    }

    // ---- the revoke list ------------------------------------------------

    pub fn device_list(&self) -> Result<DeviceListResult, RpcError> {
        let local = &self.identity.local_device;
        let mut devices = vec![PairedDeviceView {
            device_id: local.device_id.clone(),
            name: local.name.clone(),
            role: local.role,
            fingerprint: self.identity.host_fingerprint(),
            paired_via: "local".to_string(),
            // Nothing was compared: this device spawned the host it is talking
            // to. `pairing-security-mobile.md` says to persist it as device #1
            // anyway so MVP2 is not a special case, and saying so is more
            // honest than inventing a number it never showed anyone.
            sas: "—".to_string(),
            created_at: local.created_at.clone(),
            last_seen_at: None,
            revoked_at: None,
            local: true,
            // Not "is this the caller": with more than one client on one host
            // (#29) the console asking this question wants to know whether the
            // phone is up, and the phone is not the connection that asked.
            connected: self.device_is_connected(&local.device_id),
        }];

        if let Some(store) = self.store.as_ref() {
            let rows = store
                .list_paired_devices()
                .map_err(|e| RpcError::Internal(e.to_string()))?;
            for row in rows {
                devices.push(PairedDeviceView {
                    connected: self.device_is_connected(&row.device_id),
                    // A row whose role does not parse is shown as the narrow
                    // one, matching how it is enforced.
                    role: DeviceRole::parse(&row.role).unwrap_or(DeviceRole::Approver),
                    device_id: row.device_id,
                    name: row.name,
                    fingerprint: row.fingerprint,
                    paired_via: row.paired_via,
                    sas: row.sas,
                    created_at: row.created_at,
                    last_seen_at: row.last_seen_at,
                    revoked_at: row.revoked_at,
                    local: false,
                });
            }
        }
        Ok(DeviceListResult { devices })
    }

    /// Cut a device off. Durable before it is reported, and in force on that
    /// device's very next call — [`HostSession::connected_grant`] re-reads the
    /// row rather than trusting what hello decided.
    pub fn device_revoke(
        &mut self,
        params: DeviceRefParams,
    ) -> Result<DeviceRevokeResult, RpcError> {
        if params.device_id == self.identity.local_device.device_id {
            return Err(RpcError::InvalidParams(
                "the local device cannot be revoked; it is the host's own console".into(),
            ));
        }
        let store = self.store.as_ref().ok_or(RpcError::StoreUnavailable)?;
        let revoked = store
            .revoke_paired_device(&params.device_id)
            .map_err(|e| RpcError::Internal(e.to_string()))?;
        let revoked_at = store
            .get_paired_device(&params.device_id)
            .map_err(|e| RpcError::Internal(e.to_string()))?
            .and_then(|row| row.revoked_at);

        if revoked {
            // Belt and braces: the tombstone is what enforces the revoke, and
            // this makes the token unusable even to a future bug that forgets
            // to check it.
            let account = token_account(&params.device_id);
            if let Err(err) = self.secrets.delete(&account) {
                eprintln!(
                    "revoked {} but could not clear its token: {err}",
                    params.device_id
                );
            }
        }
        Ok(DeviceRevokeResult {
            device_id: params.device_id,
            revoked,
            revoked_at,
        })
    }

    // ---- what the rest of the host asks ---------------------------------

    /// Verify a paired device's `host/hello`, or refuse it.
    ///
    /// Every refusal is the same [`RpcError::UnpairedDevice`] the host has
    /// always returned for a device it does not know: whether an id is unknown,
    /// revoked, missing its token or replaying an old proof is not something a
    /// caller gets to learn by asking.
    pub(crate) fn authenticate_paired_device(
        &mut self,
        device_id: &str,
        auth: Option<&DeviceAuth>,
    ) -> Result<DeviceInfo, RpcError> {
        let store = self.store.as_ref().ok_or(RpcError::UnpairedDevice)?;
        let row = store
            .get_paired_device(device_id)
            .map_err(|e| RpcError::Internal(e.to_string()))?
            .filter(|row| !row.is_revoked())
            .ok_or(RpcError::UnpairedDevice)?;
        let role = DeviceRole::parse(&row.role).ok_or(RpcError::UnpairedDevice)?;

        let auth = auth.ok_or(RpcError::UnpairedDevice)?;
        let counter = i64::try_from(auth.counter).map_err(|_| RpcError::UnpairedDevice)?;
        // Fails closed where the vault cannot produce the token — a keychain
        // that is locked or absent means this host cannot check the proof, and
        // an unchecked proof is not a proof.
        let token = self
            .secrets
            .get(&row.token_ref)
            .map_err(|_| RpcError::UnpairedDevice)?;
        let expected = hex(&hmac_sha256(
            token.as_bytes(),
            &transcript_hash(&[
                HELLO_DOMAIN,
                &self.identity.host_id,
                device_id,
                &PROTOCOL_VERSION.to_string(),
                &auth.counter.to_string(),
            ]),
        ));
        if !ct_eq(auth.mac.as_bytes(), expected.as_bytes()) {
            return Err(RpcError::UnpairedDevice);
        }
        // The replay guard, and the check-and-write in one statement so two
        // connections cannot both spend the same counter.
        let store = self.store.as_ref().ok_or(RpcError::UnpairedDevice)?;
        if !store
            .bump_device_auth_counter(device_id, counter)
            .map_err(|e| RpcError::Internal(e.to_string()))?
        {
            return Err(RpcError::UnpairedDevice);
        }

        Ok(DeviceInfo {
            device_id: row.device_id,
            name: row.name,
            role,
            created_at: Some(row.created_at),
        })
    }

    /// What the connected device may do, straight from the store.
    ///
    /// `None` means nobody has said hello yet, which `require_hello` deals
    /// with per method. A paired device whose row has gone or been revoked
    /// since it connected is refused here — that is what makes revoke immediate
    /// rather than "immediate at next connect".
    pub(crate) fn connected_grant(&self) -> Result<Option<DeviceRole>, RpcError> {
        let Some(device_id) = self.connected_device_id.as_deref() else {
            return Ok(None);
        };
        if device_id == self.identity.local_device.device_id {
            return Ok(Some(self.identity.local_device.role));
        }
        let store = self.store.as_ref().ok_or(RpcError::UnpairedDevice)?;
        let row = store
            .get_paired_device(device_id)
            .map_err(|e| RpcError::Internal(e.to_string()))?
            .filter(|row| !row.is_revoked())
            .ok_or(RpcError::UnpairedDevice)?;
        DeviceRole::parse(&row.role)
            .map(Some)
            .ok_or(RpcError::UnpairedDevice)
    }

    /// The guard every request goes through (`router::handle`).
    pub(crate) fn require_device_scope(&self, method: &str) -> Result<(), RpcError> {
        // The handshake itself has to answer a device that cannot possibly
        // have a grant yet — that is what it is for.
        if scope::is_unauthenticated(method) && self.connected_device_id.is_none() {
            return Ok(());
        }
        let Some(role) = self.connected_grant()? else {
            return Ok(());
        };
        if scope::allows(role, method) {
            Ok(())
        } else {
            Err(RpcError::DeviceScope {
                role: role.as_str(),
                method: method.to_string(),
            })
        }
    }

    /// Forget offers nobody can use any more.
    ///
    /// Called on the way into every pairing method rather than on a timer: an
    /// expired offer must be indistinguishable from one that never existed,
    /// and doing the sweep on the read path means there is no window where a
    /// stale offer is still answerable because a tick has not fired.
    fn prune_offers(&mut self) {
        let now = Utc::now();
        self.pairing
            .offers
            .retain(|_, entry| !entry.is_expired(now) && !entry.is_spent());
    }
}

/// Nothing about a device may be blank: every one of these fields goes into
/// the transcript, and an empty field is a field that carries no commitment.
fn validate_device(device: &PairingDevice) -> Result<(), RpcError> {
    for (label, value) in [
        ("deviceId", &device.device_id),
        ("name", &device.name),
        ("fingerprint", &device.fingerprint),
        ("nonce", &device.nonce),
    ] {
        if value.trim().is_empty() {
            return Err(RpcError::InvalidParams(format!(
                "device.{label} is required"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::methods::{HelloDevice, HelloParams};

    /// A phone, implemented from the documented derivations only.
    ///
    /// Deliberately its own small implementation rather than a call into the
    /// host's helpers: the claim this module makes is that two independent
    /// programs, holding only the out-of-band secret, arrive at the *same*
    /// safety number. A test that asked the host to check its own arithmetic
    /// would assert nothing about that.
    struct Phone {
        device_id: String,
        name: String,
        key_material: String,
        nonce: String,
    }

    impl Phone {
        fn new(name: &str) -> Self {
            Self {
                device_id: Uuid::new_v4().to_string(),
                name: name.to_string(),
                key_material: random_token(),
                nonce: random_token(),
            }
        }

        fn device(&self) -> PairingDevice {
            PairingDevice {
                device_id: self.device_id.clone(),
                name: self.name.clone(),
                fingerprint: crypto::fingerprint("jabot/device-fingerprint/v1", &self.key_material),
                nonce: self.nonce.clone(),
            }
        }

        /// Everything the phone derives from a scanned QR plus its own key
        /// material. `key` is the out-of-band credential.
        fn derive(&self, qr: &PairingQr, key: &str, via: Channel) -> Derived {
            let transcript_hex = hex(&transcript_hash(&[
                TRANSCRIPT_DOMAIN,
                &qr.host_id,
                &qr.host_fingerprint,
                &qr.host_nonce,
                &qr.pairing_id,
                &self.device_id,
                &self.device().fingerprint,
                &self.nonce,
                via.as_str(),
            ]));
            Derived {
                claim_mac: hex(&bind(key, CLAIM_DOMAIN, &transcript_hex)),
                host_mac: hex(&bind(key, HOST_DOMAIN, &transcript_hex)),
                confirm_mac: hex(&bind(key, CONFIRM_DOMAIN, &transcript_hex)),
                sas: sas_digits(&bind(key, SAS_DOMAIN, &transcript_hex)),
                token: base64url(&bind(key, TOKEN_DOMAIN, &transcript_hex)),
            }
        }

        /// The `host/hello` proof, from the token pairing derived.
        fn hello_auth(&self, host_id: &str, token: &str, counter: u64) -> DeviceAuth {
            DeviceAuth {
                counter,
                mac: hex(&hmac_sha256(
                    token.as_bytes(),
                    &transcript_hash(&[
                        HELLO_DOMAIN,
                        host_id,
                        &self.device_id,
                        &PROTOCOL_VERSION.to_string(),
                        &counter.to_string(),
                    ]),
                )),
            }
        }
    }

    struct Derived {
        claim_mac: String,
        host_mac: String,
        confirm_mac: String,
        sas: String,
        token: String,
    }

    /// A host with a real store and a vault that works in-process.
    ///
    /// `secrets` is private to `host`, and this module is inside it — so the
    /// vault can be swapped here without an environment variable and without
    /// widening anything on `HostSession`.
    fn host() -> (tempfile::TempDir, HostSession) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = HostSession::load(dir.path());
        session.secrets = super::super::store::Secrets::memory();
        session
            .hello(HelloParams::default())
            .expect("the local console is implicitly paired");
        (dir, session)
    }

    fn qr_of(start: &PairingStartResult) -> PairingQr {
        serde_json::from_str(&start.qr_payload).expect("the QR payload is the offer")
    }

    /// The whole flow, both confirmations, ending in a device that can connect.
    #[test]
    fn a_scanned_qr_pairs_a_phone_and_the_phone_can_then_say_hello() {
        let (_dir, mut session) = host();
        let phone = Phone::new("Jabree's iPhone");

        let start = session
            .pairing_start(PairingStartParams::default())
            .expect("start");
        let qr = qr_of(&start);
        assert_eq!(qr.host_id, session.identity.host_id);
        assert_eq!(qr.host_fingerprint, session.identity.host_fingerprint());

        let derived = phone.derive(&qr, &qr.secret, Channel::Qr);
        let claim = session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: Some(qr.secret.clone()),
                code: None,
                device: phone.device(),
                mac: derived.claim_mac.clone(),
            })
            .expect("claim");
        // The host proves it holds the secret from its own screen, which is
        // what tells the phone it is not talking to a relay.
        assert_eq!(claim.host_mac, derived.host_mac);
        assert_eq!(claim.state, "awaiting_device");

        // Both screens, independently. This equality is the whole feature.
        let on_the_host = session.pairing_status().expect("status").offers[0]
            .sas
            .clone()
            .expect("a claimed offer has a safety number");
        assert_eq!(on_the_host, derived.sas);

        let after_device = session
            .pairing_confirm(PairingConfirmParams {
                pairing_id: qr.pairing_id.clone(),
                side: PairingSide::Device,
                sas: derived.sas.clone(),
                role: None,
                name: None,
                mac: Some(derived.confirm_mac.clone()),
            })
            .expect("device confirm");
        assert_eq!(after_device.state, "awaiting_host");
        assert!(
            after_device.device.is_none(),
            "one confirmation is not a pairing"
        );

        let paired = session
            .pairing_confirm(PairingConfirmParams {
                pairing_id: qr.pairing_id.clone(),
                side: PairingSide::Host,
                sas: derived.sas.clone(),
                role: Some(DeviceRole::Approver),
                name: None,
                mac: None,
            })
            .expect("host confirm");
        assert_eq!(paired.state, "paired");
        let device = paired.device.expect("a completed pairing names the device");
        assert_eq!(device.device_id, phone.device_id);
        assert_eq!(device.role, DeviceRole::Approver);

        // And now the thing pairing is *for*: this device can connect, with
        // the role the human on the host chose.
        let hello = session
            .hello(HelloParams {
                protocol_version: Some(PROTOCOL_VERSION),
                device: Some(HelloDevice {
                    device_id: Some(phone.device_id.clone()),
                    name: Some(phone.name.clone()),
                    role: Some(DeviceRole::Full),
                }),
                auth: Some(phone.hello_auth(&session.identity.host_id, &derived.token, 1)),
            })
            .expect("a paired device is admitted");
        // The role it asked for is not the role it got.
        assert_eq!(hello.device.role, DeviceRole::Approver);
        assert_eq!(hello.device.device_id, phone.device_id);
    }

    /// The man in the middle: someone who saw the QR and interposed their own
    /// device. Both handshakes verify — they have the secret — but the two
    /// humans are looking at different numbers, and the confirmation is where
    /// that stops being a curiosity and becomes a refusal.
    #[test]
    fn substituted_key_material_changes_the_safety_number_and_blocks_the_pairing() {
        let (_dir, mut session) = host();
        let honest = Phone::new("Jabree's iPhone");
        let attacker = Phone::new("Jabree's iPhone");

        let start = session
            .pairing_start(PairingStartParams::default())
            .expect("start");
        let qr = qr_of(&start);

        let honest_view = honest.derive(&qr, &qr.secret, Channel::Qr);
        let attacker_view = attacker.derive(&qr, &qr.secret, Channel::Qr);
        assert_ne!(
            honest_view.sas, attacker_view.sas,
            "a safety number that ignored the device's key material would be theatre"
        );

        // The attacker completes the wire half of the handshake.
        session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: Some(qr.secret.clone()),
                code: None,
                device: attacker.device(),
                mac: attacker_view.claim_mac.clone(),
            })
            .expect("the attacker holds the secret, so the MAC verifies");

        // The human on the host is reading the number off the phone in their
        // hand, which is the honest one.
        let err = session
            .pairing_confirm(PairingConfirmParams {
                pairing_id: qr.pairing_id.clone(),
                side: PairingSide::Host,
                sas: honest_view.sas.clone(),
                role: Some(DeviceRole::Approver),
                name: None,
                mac: None,
            })
            .expect_err("the numbers do not match");
        match err {
            RpcError::PairingFailed { reason, .. } => assert_eq!(reason, "sas"),
            other => panic!("expected a safety-number refusal, got {other:?}"),
        }
        assert!(
            session.device_list().expect("list").devices.len() == 1,
            "nothing may be paired but the local console"
        );
    }

    /// Without the secret there is nothing to attempt: the MAC check fails
    /// before any number is shown, and three tries close the offer.
    #[test]
    fn an_offer_is_burned_by_wrong_guesses() {
        let (_dir, mut session) = host();
        let phone = Phone::new("Somebody else's phone");
        let start = session
            .pairing_start(PairingStartParams::default())
            .expect("start");
        let qr = qr_of(&start);

        for attempt in 1..=offer::MAX_ATTEMPTS {
            let err = session
                .pairing_claim(PairingClaimParams {
                    pairing_id: qr.pairing_id.clone(),
                    secret: None,
                    code: Some("00000000".into()),
                    device: phone.device(),
                    mac: "00".repeat(32),
                })
                .expect_err("wrong credential");
            match err {
                RpcError::PairingFailed { reason, .. } => assert_eq!(reason, "credential"),
                other => panic!("attempt {attempt}: {other:?}"),
            }
        }
        // Spent. Even the right secret is now worthless.
        let derived = phone.derive(&qr, &qr.secret, Channel::Qr);
        let err = session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: Some(qr.secret.clone()),
                code: None,
                device: phone.device(),
                mac: derived.claim_mac,
            })
            .expect_err("the offer is gone");
        match err {
            RpcError::PairingFailed { reason, .. } => assert_eq!(reason, "unknown"),
            other => panic!("expected the offer to be gone, got {other:?}"),
        }
    }

    /// The screenshot case: a QR that is photographed and used later. The
    /// offer is single-use for a *different* device and worthless once the
    /// pairing completes.
    #[test]
    fn a_replayed_qr_cannot_pair_a_second_device() {
        let (_dir, mut session) = host();
        let first = Phone::new("iPhone");
        let second = Phone::new("iPad");
        let start = session
            .pairing_start(PairingStartParams::default())
            .expect("start");
        let qr = qr_of(&start);

        let first_view = first.derive(&qr, &qr.secret, Channel::Qr);
        session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: Some(qr.secret.clone()),
                code: None,
                device: first.device(),
                mac: first_view.claim_mac.clone(),
            })
            .expect("first claim");

        // Same photograph, different device.
        let second_view = second.derive(&qr, &qr.secret, Channel::Qr);
        let err = session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: Some(qr.secret.clone()),
                code: None,
                device: second.device(),
                mac: second_view.claim_mac,
            })
            .expect_err("already claimed");
        match err {
            RpcError::PairingFailed { reason, .. } => assert_eq!(reason, "claimed"),
            other => panic!("expected the offer to be used up, got {other:?}"),
        }

        // The first device's own retry is not a second use.
        let retry = session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: Some(qr.secret.clone()),
                code: None,
                device: first.device(),
                mac: first_view.claim_mac.clone(),
            })
            .expect("a retry by the same device is idempotent");
        assert_eq!(retry.host_mac, first_view.host_mac);
    }

    /// The QR is worth nothing after its window, without anyone touching it.
    #[test]
    fn an_expired_offer_is_indistinguishable_from_one_that_never_existed() {
        let (_dir, mut session) = host();
        let phone = Phone::new("iPhone");
        let start = session
            .pairing_start(PairingStartParams {
                ttl_secs: Some(offer::MIN_TTL_SECS as u64),
            })
            .expect("start");
        let qr = qr_of(&start);

        // Reach in and age the offer rather than sleeping: the property is
        // "past its expiry", not "after a wall-clock wait".
        if let Some(entry) = session.pairing.offers.get_mut(&qr.pairing_id) {
            entry.expires_at = Utc::now() - chrono::Duration::seconds(1);
        }

        let derived = phone.derive(&qr, &qr.secret, Channel::Qr);
        let err = session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: Some(qr.secret.clone()),
                code: None,
                device: phone.device(),
                mac: derived.claim_mac,
            })
            .expect_err("expired");
        match err {
            RpcError::PairingFailed { reason, .. } => assert_eq!(reason, "unknown"),
            other => panic!("expected the offer to be gone, got {other:?}"),
        }
        assert!(session.pairing_status().expect("status").offers.is_empty());
    }

    /// The headless path: a code read aloud instead of a QR scanned. Same
    /// handshake, different key — and the safety numbers differ between the
    /// channels, so a downgrade is not silent.
    #[test]
    fn a_typed_code_pairs_and_is_not_interchangeable_with_the_qr() {
        let (_dir, mut session) = host();
        let phone = Phone::new("iPhone");
        let start = session
            .pairing_start(PairingStartParams::default())
            .expect("start");
        let qr = qr_of(&start);

        let by_code = phone.derive(&qr, &start.code, Channel::Code);
        let by_qr = phone.derive(&qr, &qr.secret, Channel::Qr);
        assert_ne!(by_code.sas, by_qr.sas);

        // Typed the way a human types it.
        let typed = format!("{}-{}", start.code[..4].to_lowercase(), &start.code[4..]);
        let claim = session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: None,
                code: Some(typed),
                device: phone.device(),
                mac: by_code.claim_mac.clone(),
            })
            .expect("claim by code");
        assert_eq!(claim.via, "code");
        assert_eq!(claim.host_mac, by_code.host_mac);
    }

    /// The grant is a role, and the role is enforced against the row.
    #[test]
    fn an_approver_is_scoped_and_a_revoke_lands_on_the_next_call() {
        use crate::host::protocol::methods::{PERMISSION_REPLY, THREAD_DELETE};

        let (_dir, mut session) = host();
        let phone = Phone::new("iPhone");
        let start = session
            .pairing_start(PairingStartParams::default())
            .expect("start");
        let qr = qr_of(&start);
        let derived = phone.derive(&qr, &qr.secret, Channel::Qr);
        session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: Some(qr.secret.clone()),
                code: None,
                device: phone.device(),
                mac: derived.claim_mac.clone(),
            })
            .expect("claim");
        for side in [PairingSide::Device, PairingSide::Host] {
            session
                .pairing_confirm(PairingConfirmParams {
                    pairing_id: qr.pairing_id.clone(),
                    side,
                    sas: derived.sas.clone(),
                    role: Some(DeviceRole::Approver),
                    name: Some("Jabree's iPhone".into()),
                    mac: Some(derived.confirm_mac.clone()),
                })
                .expect("confirm");
        }

        session
            .hello(HelloParams {
                protocol_version: Some(PROTOCOL_VERSION),
                device: Some(HelloDevice {
                    device_id: Some(phone.device_id.clone()),
                    name: None,
                    role: None,
                }),
                auth: Some(phone.hello_auth(&session.identity.host_id, &derived.token, 1)),
            })
            .expect("connect");

        // What a phone is for, and what it is not.
        assert!(session.require_device_scope(PERMISSION_REPLY).is_ok());
        match session.require_device_scope(THREAD_DELETE) {
            Err(RpcError::DeviceScope { role, .. }) => assert_eq!(role, "approver"),
            other => panic!("a phone must not be able to delete a thread: {other:?}"),
        }

        // Revoked from the desktop while the phone is still connected. The
        // next call it makes has to fail — not the next time it reconnects.
        let store = session.store().expect("store");
        assert!(store
            .revoke_paired_device(&phone.device_id)
            .expect("revoke"));
        match session.require_device_scope(PERMISSION_REPLY) {
            Err(RpcError::UnpairedDevice) => {}
            other => panic!("a revoked device must be refused at once: {other:?}"),
        }
    }

    /// Revocation is durable, and it is not a re-pairing hazard: the token is
    /// gone, so the old proof cannot be replayed even by the same device id.
    #[test]
    fn a_revoked_device_cannot_come_back_with_its_old_proof() {
        let (dir, mut session) = host();
        let phone = Phone::new("iPhone");
        let start = session
            .pairing_start(PairingStartParams::default())
            .expect("start");
        let qr = qr_of(&start);
        let derived = phone.derive(&qr, &qr.secret, Channel::Qr);
        session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: Some(qr.secret.clone()),
                code: None,
                device: phone.device(),
                mac: derived.claim_mac.clone(),
            })
            .expect("claim");
        for side in [PairingSide::Device, PairingSide::Host] {
            session
                .pairing_confirm(PairingConfirmParams {
                    pairing_id: qr.pairing_id.clone(),
                    side,
                    sas: derived.sas.clone(),
                    role: Some(DeviceRole::Full),
                    name: None,
                    mac: Some(derived.confirm_mac.clone()),
                })
                .expect("confirm");
        }

        let revoke = session
            .device_revoke(DeviceRefParams {
                device_id: phone.device_id.clone(),
            })
            .expect("revoke");
        assert!(revoke.revoked);
        assert!(revoke.revoked_at.is_some());

        let auth = phone.hello_auth(&session.identity.host_id, &derived.token, 2);
        match session.hello(HelloParams {
            protocol_version: Some(PROTOCOL_VERSION),
            device: Some(HelloDevice {
                device_id: Some(phone.device_id.clone()),
                name: None,
                role: None,
            }),
            auth: Some(auth.clone()),
        }) {
            Err(RpcError::UnpairedDevice) => {}
            other => panic!("a revoked device must not connect: {other:?}"),
        }

        // And it is still refused by a host that was restarted — the tombstone
        // is on disk, not in this process.
        drop(session);
        let mut restarted = HostSession::load(dir.path());
        restarted.secrets = super::super::store::Secrets::memory();
        match restarted.hello(HelloParams {
            protocol_version: Some(PROTOCOL_VERSION),
            device: Some(HelloDevice {
                device_id: Some(phone.device_id.clone()),
                name: None,
                role: None,
            }),
            auth: Some(auth),
        }) {
            Err(RpcError::UnpairedDevice) => {}
            other => panic!("a restart must not forget a revoke: {other:?}"),
        }
        let listed = restarted.device_list().expect("list");
        let row = listed
            .devices
            .iter()
            .find(|d| d.device_id == phone.device_id)
            .expect("the revoke list keeps the row");
        assert!(row.revoked_at.is_some());
    }

    /// The replay guard on the connection proof, which is what makes the
    /// `deviceId` on the wire worth anything without transport confidentiality.
    #[test]
    fn a_captured_hello_proof_cannot_be_replayed() {
        let (_dir, mut session) = host();
        let phone = Phone::new("iPhone");
        let start = session
            .pairing_start(PairingStartParams::default())
            .expect("start");
        let qr = qr_of(&start);
        let derived = phone.derive(&qr, &qr.secret, Channel::Qr);
        session
            .pairing_claim(PairingClaimParams {
                pairing_id: qr.pairing_id.clone(),
                secret: Some(qr.secret.clone()),
                code: None,
                device: phone.device(),
                mac: derived.claim_mac.clone(),
            })
            .expect("claim");
        for side in [PairingSide::Device, PairingSide::Host] {
            session
                .pairing_confirm(PairingConfirmParams {
                    pairing_id: qr.pairing_id.clone(),
                    side,
                    sas: derived.sas.clone(),
                    role: Some(DeviceRole::Full),
                    name: None,
                    mac: Some(derived.confirm_mac.clone()),
                })
                .expect("confirm");
        }

        let hello_of = |counter: u64| HelloParams {
            protocol_version: Some(PROTOCOL_VERSION),
            device: Some(HelloDevice {
                device_id: Some(phone.device_id.clone()),
                name: None,
                role: None,
            }),
            auth: Some(phone.hello_auth(&session.identity.host_id, &derived.token, counter)),
        };
        let host_id = session.identity.host_id.clone();
        let five = HelloParams {
            auth: Some(phone.hello_auth(&host_id, &derived.token, 5)),
            ..hello_of(5)
        };
        session.hello(five.clone()).expect("first connection");
        match session.hello(five) {
            Err(RpcError::UnpairedDevice) => {}
            other => panic!("the same proof must not work twice: {other:?}"),
        }
        // A proof with no counter at all is not a proof either.
        match session.hello(HelloParams {
            protocol_version: Some(PROTOCOL_VERSION),
            device: Some(HelloDevice {
                device_id: Some(phone.device_id.clone()),
                name: None,
                role: None,
            }),
            auth: None,
        }) {
            Err(RpcError::UnpairedDevice) => {}
            other => panic!("a device id on its own is not a credential: {other:?}"),
        }
    }

    /// The local console is device #1 and stays that way.
    #[test]
    fn the_local_device_is_listed_and_cannot_be_revoked() {
        let (_dir, mut session) = host();
        let local = session.identity.local_device.device_id.clone();
        let listed = session.device_list().expect("list");
        assert_eq!(listed.devices.len(), 1);
        assert!(listed.devices[0].local);
        assert!(listed.devices[0].connected);
        assert_eq!(listed.devices[0].role, DeviceRole::Full);

        match session.device_revoke(DeviceRefParams { device_id: local }) {
            Err(RpcError::InvalidParams(_)) => {}
            other => panic!("the desktop must not be able to lock itself out: {other:?}"),
        }
    }

    /// A host with nowhere to record a grant says so before anyone walks to
    /// their phone, rather than after the safety numbers have been compared.
    #[test]
    fn an_ephemeral_host_refuses_to_offer_a_pairing() {
        let mut session = HostSession::ephemeral();
        session.hello(HelloParams::default()).expect("hello");
        match session.pairing_start(PairingStartParams::default()) {
            Err(RpcError::StoreUnavailable) => {}
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// The credentials are handed out once, by the call that creates them.
    #[test]
    fn a_status_listing_never_hands_back_a_live_credential() {
        let (_dir, mut session) = host();
        let start = session
            .pairing_start(PairingStartParams::default())
            .expect("start");
        let listed =
            serde_json::to_string(&session.pairing_status().expect("status")).expect("serialize");
        assert!(!listed.contains(&start.secret));
        assert!(!listed.contains(&start.code));
    }
}
