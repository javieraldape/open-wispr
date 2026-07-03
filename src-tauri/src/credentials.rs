use keyring::{Entry, Error as KeyringError};
use log::warn;

use crate::settings::AppSettings;

pub(crate) const STORED_API_KEY_MARKER: &str = "[KEY STORED]";

const POST_PROCESS_API_KEY_SERVICE: &str = "com.openwispr.app.post-process-api-key";

fn entry(provider_id: &str) -> Result<Entry, String> {
    Entry::new(POST_PROCESS_API_KEY_SERVICE, provider_id)
        .map_err(|e| format!("Failed to open OS credential store: {}", e))
}

pub(crate) fn set_post_process_api_key(provider_id: &str, api_key: &str) -> Result<(), String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return delete_post_process_api_key(provider_id);
    }

    entry(provider_id)?
        .set_password(api_key)
        .map_err(|e| format!("Failed to save API key in OS credential store: {}", e))
}

pub(crate) fn get_post_process_api_key(provider_id: &str) -> Result<String, String> {
    match entry(provider_id)?.get_password() {
        Ok(api_key) => Ok(api_key),
        Err(KeyringError::NoEntry) => Ok(String::new()),
        Err(e) => Err(format!(
            "Failed to read API key from OS credential store: {}",
            e
        )),
    }
}

pub(crate) fn delete_post_process_api_key(provider_id: &str) -> Result<(), String> {
    match entry(provider_id)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(e) => Err(format!(
            "Failed to delete API key from OS credential store: {}",
            e
        )),
    }
}

pub(crate) fn marker_for_api_key(api_key: &str) -> String {
    if api_key.trim().is_empty() {
        String::new()
    } else {
        STORED_API_KEY_MARKER.to_string()
    }
}

pub(crate) fn sanitize_post_process_api_key_settings(settings: &mut AppSettings) -> bool {
    let mut changed = false;
    for (provider_id, value) in settings.post_process_api_keys.iter_mut() {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            if !value.is_empty() {
                *value = String::new();
                changed = true;
            }
            continue;
        }

        if trimmed == STORED_API_KEY_MARKER {
            if value.as_str() != STORED_API_KEY_MARKER {
                *value = STORED_API_KEY_MARKER.to_string();
                changed = true;
            }
            continue;
        }

        match set_post_process_api_key(provider_id, &trimmed) {
            Ok(()) => {
                *value = STORED_API_KEY_MARKER.to_string();
                changed = true;
            }
            Err(e) => {
                warn!(
                    "Could not migrate post-process API key for provider '{}' to OS credential store: {}",
                    provider_id, e
                );
                *value = String::new();
                changed = true;
            }
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    fn use_mock_keyring() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
    }

    #[test]
    fn sanitize_migrates_plaintext_keys_to_keyring_markers() {
        use_mock_keyring();
        let provider_id = format!(
            "test-provider-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let mut settings = get_default_settings();
        settings
            .post_process_api_keys
            .insert(provider_id.clone(), "sk-test-secret".to_string());

        assert!(sanitize_post_process_api_key_settings(&mut settings));
        assert_eq!(
            settings
                .post_process_api_keys
                .get(&provider_id)
                .map(String::as_str),
            Some(STORED_API_KEY_MARKER)
        );
    }

    #[test]
    fn marker_for_empty_api_key_stays_empty() {
        assert_eq!(marker_for_api_key(""), "");
        assert_eq!(marker_for_api_key("  "), "");
        assert_eq!(marker_for_api_key("secret"), STORED_API_KEY_MARKER);
    }
}
