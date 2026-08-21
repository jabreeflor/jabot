//! One pairing offer: the short-lived, single-use thing a QR encodes.
//!
//! Offers live in RAM and nowhere else. That is a security decision, not an
//! omission. `pairing-security-mobile.md` rule 6 says the nonce is single-use
//! precisely because a long-lived capability is how this class of product ends
//! up in a CVE writeup, and the cheapest way to keep that promise across a
//! crash is to have nothing to recover: a QR photographed off someone's screen
//! is worthless the moment the host restarts, and a secret that was never
//! written down cannot be read out of a backup. What *is* durable is the
//! result — the paired device row (`paired_devices`) — which is the half that
//! has to survive a quit.
//!
//! An offer therefore burns three ways: it expires, it is claimed and
//! completed, or it is spent on wrong guesses.

use chrono::{DateTime, Duration, Utc};

use super::super::protocol::methods::DeviceRole;

/// How long a QR is worth scanning. Seconds-to-minutes, per the research.
pub const DEFAULT_TTL_SECS: i64 = 120;
/// Bounds on a caller-supplied TTL. The floor keeps a UI from creating an
/// offer that is dead before it is drawn; the ceiling is the actual promise.
pub const MIN_TTL_SECS: i64 = 5;
pub const MAX_TTL_SECS: i64 = 600;
/// Wrong credentials before the offer is burned.
///
/// This is what makes the headless channel defensible: an eight-character
/// Crockford code is 40 bits, which is a lot to type and not a lot to grind,
/// so the offer stops answering after three tries rather than relying on the
/// code's entropy alone.
pub const MAX_ATTEMPTS: u32 = 3;
/// Offers a host will hold at once, so a client that loops on `pairing/start`
/// cannot grow the map without bound.
pub const MAX_OPEN_OFFERS: usize = 8;

/// Which out-of-band channel carried the secret.
///
/// It is part of the transcript, so a man in the middle cannot quietly
/// downgrade a scan to a typed code and have the two safety numbers still
/// agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Qr,
    Code,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qr => "qr",
            Self::Code => "code",
        }
    }
}

/// What a device said when it claimed the offer, plus what the two sides
/// derived from it.
#[derive(Debug, Clone)]
pub struct Claim {
    pub device_id: String,
    pub device_name: String,
    pub device_fingerprint: String,
    pub device_nonce: String,
    pub via: Channel,
    /// The safety number, derived from both sides' material — see
    /// [`super::transcript`].
    pub sas: String,
    /// The shared secret both sides derived and neither sent.
    pub device_token: String,
    pub host_confirmed: bool,
    pub device_confirmed: bool,
    /// Chosen by the human on the *host*, never by the claiming device.
    pub role: Option<DeviceRole>,
    /// A name the host operator typed in place of the one the device offered.
    pub name_override: Option<String>,
}

impl Claim {
    pub fn both_confirmed(&self) -> bool {
        self.host_confirmed && self.device_confirmed
    }

    pub fn granted_role(&self) -> DeviceRole {
        // Approver is the safe default if a confirm ever lands without one:
        // the failure mode of guessing low is a phone that cannot delete a
        // thread, and of guessing high is a phone that can.
        self.role.unwrap_or(DeviceRole::Approver)
    }

    pub fn display_name(&self) -> String {
        self.name_override
            .clone()
            .unwrap_or_else(|| self.device_name.clone())
    }
}

#[derive(Debug, Clone)]
pub enum OfferState {
    /// Displayed, nobody has scanned it.
    Offered,
    /// A device proved it holds the secret; both humans owe a confirmation.
    Claimed(Box<Claim>),
}

#[derive(Debug, Clone)]
pub struct Offer {
    pub id: String,
    /// The QR channel credential: 256 bits, base64url.
    pub secret: String,
    /// The typed channel credential, already normalized.
    pub code: String,
    pub host_nonce: String,
    pub expires_at: DateTime<Utc>,
    pub attempts: u32,
    pub state: OfferState,
}

impl Offer {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.expires_at
    }

    pub fn is_spent(&self) -> bool {
        self.attempts >= MAX_ATTEMPTS
    }

    /// The HMAC key for a claim on this channel.
    ///
    /// The key *is* the out-of-band credential, so a proof made with the typed
    /// code cannot be replayed as a proof of having scanned the QR, and the
    /// safety number differs between the two channels. It is also why neither
    /// credential is ever asked for on the wire: `pairing/claim` tries this
    /// key for each channel and keeps whichever one the device's MAC verifies
    /// under, which proves possession without transporting the thing possessed.
    pub fn channel_key(&self, via: Channel) -> &str {
        match via {
            Channel::Qr => &self.secret,
            Channel::Code => &self.code,
        }
    }

    pub fn claim(&self) -> Option<&Claim> {
        match &self.state {
            OfferState::Claimed(claim) => Some(claim),
            OfferState::Offered => None,
        }
    }

    pub fn claim_mut(&mut self) -> Option<&mut Claim> {
        match &mut self.state {
            OfferState::Claimed(claim) => Some(claim),
            OfferState::Offered => None,
        }
    }

    /// What `pairing/status` shows the host operator.
    pub fn state_name(&self) -> &'static str {
        match self.claim() {
            None => "offered",
            Some(claim) if claim.both_confirmed() => "paired",
            Some(claim) if claim.device_confirmed => "awaiting_host",
            Some(_) => "awaiting_device",
        }
    }
}

/// Clamp a caller's TTL, or fall back to the environment and then the default.
///
/// The environment override exists for the same reason `LifecycleState` has
/// one: a test that wants to watch an offer expire should not have to sleep
/// for two minutes.
pub fn ttl_seconds(requested: Option<u64>) -> i64 {
    let from_env = std::env::var("JABOT_PAIRING_TTL_SECS")
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok());
    let wanted = requested
        .and_then(|secs| i64::try_from(secs).ok())
        .or(from_env)
        .unwrap_or(DEFAULT_TTL_SECS);
    wanted.clamp(MIN_TTL_SECS, MAX_TTL_SECS)
}

pub fn expiry(created_at: DateTime<Utc>, ttl_secs: i64) -> DateTime<Utc> {
    created_at + Duration::seconds(ttl_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> Offer {
        let now = Utc::now();
        Offer {
            id: "offer-1".into(),
            secret: "s3cr3t-material".into(),
            code: "ABCD1234".into(),
            host_nonce: "host-nonce".into(),
            expires_at: expiry(now, DEFAULT_TTL_SECS),
            attempts: 0,
            state: OfferState::Offered,
        }
    }

    #[test]
    fn the_channel_decides_the_key() {
        let offer = offer();
        assert_eq!(offer.channel_key(Channel::Qr), "s3cr3t-material");
        assert_eq!(offer.channel_key(Channel::Code), "ABCD1234");
    }

    #[test]
    fn an_offer_expires_and_can_be_spent() {
        let mut offer = offer();
        assert!(!offer.is_expired(Utc::now()));
        assert!(offer.is_expired(offer.expires_at));
        assert!(offer.is_expired(offer.expires_at + Duration::seconds(1)));

        offer.attempts = MAX_ATTEMPTS;
        assert!(offer.is_spent());
    }

    #[test]
    fn ttl_is_clamped_rather_than_trusted() {
        assert_eq!(ttl_seconds(Some(30)), 30);
        assert_eq!(ttl_seconds(Some(0)), MIN_TTL_SECS);
        assert_eq!(ttl_seconds(Some(86_400)), MAX_TTL_SECS);
    }

    #[test]
    fn state_name_tracks_both_confirmations() {
        let mut offer = offer();
        assert_eq!(offer.state_name(), "offered");
        offer.state = OfferState::Claimed(Box::new(Claim {
            device_id: "d1".into(),
            device_name: "iPhone".into(),
            device_fingerprint: "fp".into(),
            device_nonce: "n".into(),
            via: Channel::Qr,
            sas: "1111-2222".into(),
            device_token: "token".into(),
            host_confirmed: false,
            device_confirmed: false,
            role: None,
            name_override: None,
        }));
        assert_eq!(offer.state_name(), "awaiting_device");
        if let Some(claim) = offer.claim_mut() {
            claim.device_confirmed = true;
        }
        assert_eq!(offer.state_name(), "awaiting_host");
        if let Some(claim) = offer.claim_mut() {
            claim.host_confirmed = true;
        }
        assert_eq!(offer.state_name(), "paired");
    }

    #[test]
    fn a_claim_without_a_role_is_the_narrow_one() {
        let claim = Claim {
            device_id: "d1".into(),
            device_name: "iPhone".into(),
            device_fingerprint: "fp".into(),
            device_nonce: "n".into(),
            via: Channel::Qr,
            sas: "1111-2222".into(),
            device_token: "token".into(),
            host_confirmed: false,
            device_confirmed: false,
            role: None,
            name_override: None,
        };
        assert_eq!(claim.granted_role(), DeviceRole::Approver);
    }
}
