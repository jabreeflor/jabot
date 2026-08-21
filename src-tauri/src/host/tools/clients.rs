//! Where an OAuth client id comes from when the provider does not mint one.
//!
//! Most remote MCP servers support dynamic client registration, and that is
//! the path [`super::oauth::register_client`] takes. The ones that do not
//! (Google and Slack both want an app registered in their console) need an id
//! that identifies *this installation of JaBot* — and JaBot cannot ship one:
//! a client id is issued to a registered application with its own consent
//! screen, redirect URIs and review status. Inventing a string here would
//! produce a browser page that says `invalid_client` and nothing a user can do
//! about it.
//!
//! So the id comes from the user's own registration, in
//! `<data dir>/oauth_clients.json`:
//!
//! ```json
//! {
//!   "google": { "clientId": "1234-abc.apps.googleusercontent.com" }
//! }
//! ```
//!
//! A `clientSecret` is accepted for providers that still require one, but it
//! is only as protected as that file — prefer registering a public/native
//! client, which is what PKCE exists for.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::oauth::OAuthClient;

pub const CLIENTS_FILE: &str = "oauth_clients.json";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisteredClient {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
}

/// The registration for one provider, if the user wrote one.
///
/// A malformed file is not silently ignored: it comes back as an error the
/// connect flow shows on the chip, because the user who wrote the file is the
/// only person who can fix it.
pub fn registered(dir: Option<&Path>, provider: &str) -> Result<Option<OAuthClient>, String> {
    let Some(dir) = dir else {
        return Ok(None);
    };
    let path = dir.join(CLIENTS_FILE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("could not read {}: {err}", path.display())),
    };
    let parsed: BTreeMap<String, RegisteredClient> = serde_json::from_str(&raw)
        .map_err(|err| format!("{} is not valid JSON: {err}", path.display()))?;
    Ok(parsed.get(provider).map(|client| OAuthClient {
        client_id: client.client_id.clone(),
        secret: client.client_secret.clone(),
    }))
}

/// What to tell a user who has no registration and no dynamic registration.
pub fn missing_client_hint(provider_label: &str, dir: Option<&Path>) -> String {
    let location = dir
        .map(|dir| dir.join(CLIENTS_FILE).display().to_string())
        .unwrap_or_else(|| CLIENTS_FILE.to_string());
    format!(
        "{provider_label} does not register clients automatically. \
         Register a native OAuth client with {provider_label} and put its id in {location}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_registration_and_ignores_other_providers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(CLIENTS_FILE),
            r#"{"google":{"clientId":"abc.apps.googleusercontent.com"}}"#,
        )
        .unwrap();

        let google = registered(Some(dir.path()), "google").unwrap().unwrap();
        assert_eq!(google.client_id, "abc.apps.googleusercontent.com");
        assert!(google.secret.is_none());
        assert!(registered(Some(dir.path()), "slack").unwrap().is_none());
    }

    #[test]
    fn no_file_and_no_data_dir_are_both_simply_no_registration() {
        let dir = tempfile::tempdir().unwrap();
        assert!(registered(Some(dir.path()), "google").unwrap().is_none());
        assert!(registered(None, "google").unwrap().is_none());
    }

    /// A typo in the file has to reach the user. Treating it as "no client"
    /// would send them to the console to register one they already have.
    #[test]
    fn a_broken_file_is_reported_not_swallowed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(CLIENTS_FILE), "{ not json").unwrap();
        let err = registered(Some(dir.path()), "google").unwrap_err();
        assert!(err.contains("not valid JSON"), "{err}");
    }
}
