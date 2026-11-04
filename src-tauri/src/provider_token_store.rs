//! Keep Gospel's credential locations while adapting Rig's token-store API.

use rig::error::BoxError;
use rig::providers::chatgpt::auth::{FileTokenStore, StoreLock, TokenStore};
use rig::wasm_compat::WasmBoxedFuture;

pub(crate) struct ProviderTokenStore;

impl ProviderTokenStore {
    fn store(key: &str) -> Result<(FileTokenStore, &'static str), BoxError> {
        match key {
            "chatgpt" => Ok((
                FileTokenStore::new(
                    crate::keychain::chatgpt_auth_file_path()
                        .parent()
                        .ok_or("missing credential directory")?,
                ),
                "auth",
            )),
            "copilot" => Ok((
                FileTokenStore::new(crate::keychain::github_copilot_token_dir()),
                "copilot",
            )),
            _ => Err("unsupported credential store key".into()),
        }
    }
}

impl TokenStore for ProviderTokenStore {
    fn load<'a>(&'a self, key: &'a str) -> WasmBoxedFuture<'a, Result<Option<String>, BoxError>> {
        Box::pin(async move {
            let (store, record_key) = Self::store(key)?;
            if let Some(record) = store.load(record_key).await? {
                return Ok(Some(record));
            }
            if key != "copilot" {
                return Ok(None);
            }
            let read = |path| -> Result<Option<String>, BoxError> {
                match std::fs::read_to_string(path) {
                    Ok(text) => Ok(Some(text)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error.into()),
                }
            };
            let token = read(crate::keychain::github_copilot_access_token_path())?;
            let session = read(crate::keychain::github_copilot_api_key_path())?;
            if token.is_none() && session.is_none() {
                return Ok(None);
            }
            let mut record = match session {
                Some(session) => serde_json::from_str::<serde_json::Value>(&session)?,
                None => serde_json::json!({}),
            };
            let fields = record
                .as_object_mut()
                .ok_or("invalid Copilot credential record")?;
            if let Some(token) = token.filter(|token| !token.trim().is_empty()) {
                fields.insert("github_token".into(), token.trim().into());
            }
            let record = serde_json::to_string(&record)?;
            store.save(record_key, &record).await?;
            Ok(Some(record))
        })
    }

    fn save<'a>(
        &'a self,
        key: &'a str,
        record: &'a str,
    ) -> WasmBoxedFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let (store, key) = Self::store(key)?;
            store.save(key, record).await
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> WasmBoxedFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let (store, key) = Self::store(key)?;
            store.delete(key).await
        })
    }

    fn lock<'a>(&'a self, key: &'a str) -> WasmBoxedFuture<'a, Result<StoreLock, BoxError>> {
        Box::pin(async move {
            let (store, key) = Self::store(key)?;
            store.lock(key).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrates_existing_copilot_credentials_and_logout_removes_both_formats() {
        let _lock = crate::keychain::lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = crate::keychain::isolate_config_home(dir.path());
        std::fs::create_dir_all(crate::keychain::github_copilot_token_dir()).unwrap();
        std::fs::write(
            crate::keychain::github_copilot_access_token_path(),
            "stored-github-token\n",
        )
        .unwrap();
        std::fs::write(crate::keychain::github_copilot_api_key_path(), r#"{"token":"stored-session-token","expires_at":4102444800,"endpoints":{"api":"https://api.business.githubcopilot.com"}}"#).unwrap();
        let record = ProviderTokenStore.load("copilot").await.unwrap().unwrap();
        let record: serde_json::Value = serde_json::from_str(&record).unwrap();
        assert_eq!(record["github_token"], "stored-github-token");
        assert_eq!(record["token"], "stored-session-token");
        assert_eq!(
            record["endpoints"]["api"],
            "https://api.business.githubcopilot.com"
        );
        let path = crate::keychain::github_copilot_token_dir().join("copilot.json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_file(crate::keychain::github_copilot_access_token_path()).unwrap();
        std::fs::remove_file(crate::keychain::github_copilot_api_key_path()).unwrap();
        assert!(crate::keychain::has_github_copilot_oauth_session());
        std::fs::write(
            crate::keychain::github_copilot_access_token_path(),
            "stored-github-token",
        )
        .unwrap();
        std::fs::write(
            crate::keychain::github_copilot_api_key_path(),
            r#"{"token":"stored-session-token"}"#,
        )
        .unwrap();
        crate::keychain::delete_github_copilot_auth_files().unwrap();
        assert!(!path.exists());
        assert!(!crate::keychain::github_copilot_access_token_path().exists());
        assert!(!crate::keychain::github_copilot_api_key_path().exists());
        assert!(!crate::keychain::has_github_copilot_oauth_session());
        assert!(ProviderTokenStore.load("copilot").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn chatgpt_keeps_its_existing_auth_file() {
        let _lock = crate::keychain::lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = crate::keychain::isolate_config_home(dir.path());
        ProviderTokenStore
            .save("chatgpt", r#"{"access_token":"stored-chatgpt-token"}"#)
            .await
            .unwrap();
        assert!(crate::keychain::chatgpt_auth_file_path().exists());
        assert!(crate::keychain::has_chatgpt_oauth_session());
        assert!(ProviderTokenStore
            .load("chatgpt")
            .await
            .unwrap()
            .unwrap()
            .contains("stored-chatgpt-token"));
        crate::keychain::delete_chatgpt_auth_file().unwrap();
        assert!(ProviderTokenStore.load("chatgpt").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn invalid_legacy_credentials_are_not_overwritten() {
        let _lock = crate::keychain::lock_config_home();
        let dir = tempfile::tempdir().unwrap();
        let _home = crate::keychain::isolate_config_home(dir.path());
        std::fs::create_dir_all(crate::keychain::github_copilot_token_dir()).unwrap();
        std::fs::write(
            crate::keychain::github_copilot_api_key_path(),
            "broken JSON",
        )
        .unwrap();
        assert!(ProviderTokenStore.load("copilot").await.is_err());
        assert!(!crate::keychain::github_copilot_token_dir()
            .join("copilot.json")
            .exists());
        assert_eq!(
            std::fs::read_to_string(crate::keychain::github_copilot_api_key_path()).unwrap(),
            "broken JSON"
        );
    }
}
