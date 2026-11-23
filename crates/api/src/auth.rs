//! Better Auth RS, mounted at `/api/auth` (ADR 0006, docs/11-security.md).

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use better_auth::plugins::endpoint_context::EndpointContext;
use better_auth::plugins::user_admission::{
    UserValidationAction, UserValidationData, UserValidationRejection, ValidateUserInfo,
};
use better_auth::plugins::{EmailPasswordPlugin, SessionManagementPlugin};
use better_auth::seaorm::{DatabaseHookUpdate, SeaOrmHookContext, SeaOrmHooks, SeaOrmStore};
use better_auth::{
    Argon2PasswordHasher, AuthConfig, AuthResult, BetterAuth, FieldMap, database_hooks,
};
use db::DatabaseConnection;
use db::auth_schema::AppAuthSchema;
use hkdf::Hkdf;
use sha2::Sha256;

pub type Auth = Arc<BetterAuth<AppAuthSchema>>;

/// Session lifetime, sliding. The setting `auth.session_days`
/// (docs/10-configuration.md) replaces this constant when settings exist.
const SESSION_DAYS: i64 = 30;

/// docs/11-security.md. Set before the first user exists.
const PASSWORD_MIN_LENGTH: usize = 10;

/// Admission rules that the library asks at request time.
///
/// The state is in memory: the callbacks run inside the auth transaction,
/// and a query on our `DatabaseConnection` from there waits for the single
/// SQLite connection and times out (ADR 0006).
#[derive(Clone, Default)]
pub struct Policy {
    admit_next_signup: Arc<AtomicBool>,
    disabled: Arc<RwLock<HashSet<String>>>,
}

impl Policy {
    /// Seam for the first-run setup (#32): lets exactly one user creation
    /// pass. Public sign-up is closed otherwise.
    pub fn admit_next_signup(&self) {
        self.admit_next_signup.store(true, Ordering::SeqCst);
    }

    /// Stops new sessions for this user. The users API (#36) also deletes the
    /// existing sessions with `session_manager().revoke_all_user_sessions`,
    /// and loads this set from the `disabled` column (#28) at startup.
    pub fn disable(&self, user_id: &str) {
        self.disabled.write().unwrap().insert(user_id.to_owned());
    }
}

#[async_trait]
impl ValidateUserInfo<AppAuthSchema> for Policy {
    async fn validate(
        &self,
        data: &UserValidationData,
        _context: &EndpointContext<'_, AppAuthSchema>,
    ) -> AuthResult<Option<UserValidationRejection>> {
        Ok(match data.source.action {
            UserValidationAction::CreateUser
                if !self.admit_next_signup.swap(false, Ordering::SeqCst) =>
            {
                Some(UserValidationRejection::new("signup_closed"))
            }
            _ => None,
        })
    }
}

#[database_hooks]
impl SeaOrmHooks<AppAuthSchema> for Policy {
    /// Covers every sign-in method: without a session there is no sign-in.
    async fn before_create_session(
        &self,
        session: &mut FieldMap,
        _ctx: &SeaOrmHookContext<'_, AppAuthSchema>,
    ) -> AuthResult<DatabaseHookUpdate<FieldMap>> {
        let user_id = session
            .get("userId")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        Ok(if self.disabled.read().unwrap().contains(user_id) {
            DatabaseHookUpdate::Cancel
        } else {
            DatabaseHookUpdate::Continue
        })
    }
}

/// Builds the auth instance on the migrated database. `secret` is the root
/// secret from [`load_secret`]; `base_url` is `UNSEEN_BASE_URL`.
pub async fn build(
    db: DatabaseConnection,
    secret: &str,
    base_url: Option<&str>,
) -> AuthResult<(Auth, Policy)> {
    let mut config = AuthConfig::new(signing_key(secret))
        .session_expires_in(chrono::Duration::days(SESSION_DAYS))
        .password_min_length(PASSWORD_MIN_LENGTH);
    // Set explicitly: the library otherwise decides from `NODE_ENV`.
    config.advanced.use_secure_cookies =
        Some(base_url.is_some_and(|url| url.starts_with("https://")));
    if let Some(url) = base_url {
        config = config.base_url(url);
    }

    let policy = Policy::default();
    let store = SeaOrmStore::<AppAuthSchema>::new(config.clone(), db).hook(policy.clone());
    let auth = BetterAuth::<AppAuthSchema>::new(config)
        .store(store)
        .validate_user_info(Arc::new(policy.clone()))
        .plugin(
            // Sign-up stays enabled in the library; `Policy` closes it.
            EmailPasswordPlugin::new()
                .password_min_length(PASSWORD_MIN_LENGTH)
                .password_hasher(Arc::new(Argon2PasswordHasher)),
        )
        .plugin(SessionManagementPlugin::new())
        .build()
        .await?;
    Ok((Arc::new(auth), policy))
}

/// The root secret: `UNSEEN_SECRET` when set, else the file `secret` in the
/// data directory, created with a random value on first run.
pub fn load_secret(env: Option<String>, data_dir: &Path) -> io::Result<String> {
    if let Some(secret) = env.filter(|secret| !secret.is_empty()) {
        return Ok(secret);
    }
    let path = data_dir.join("secret");
    match fs::read_to_string(&path) {
        Ok(secret) => Ok(secret.trim().to_owned()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let secret = hex(&rand::random::<[u8; 32]>());
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            options.open(&path)?.write_all(secret.as_bytes())?;
            Ok(secret)
        }
        Err(error) => Err(error),
    }
}

/// The auth signing key, derived from the root secret so that it differs
/// from the key that encrypts integration secrets (docs/11-security.md).
fn signing_key(secret: &str) -> String {
    let mut key = [0; 32];
    Hkdf::<Sha256>::new(None, secret.as_bytes())
        .expand(b"unseen auth signing key v1", &mut key)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    hex(&key)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_from_the_environment_wins_and_writes_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let secret = load_secret(Some("from-env".into()), dir.path()).unwrap();
        assert_eq!(secret, "from-env");
        assert!(!dir.path().join("secret").exists());
    }

    #[test]
    fn secret_file_is_created_once_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_secret(None, dir.path()).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(
            fs::read_to_string(dir.path().join("secret")).unwrap(),
            first
        );
        // An empty variable counts as absent.
        assert_eq!(load_secret(Some(String::new()), dir.path()).unwrap(), first);
        assert_ne!(
            load_secret(None, tempfile::tempdir().unwrap().path()).unwrap(),
            first
        );
    }

    #[test]
    fn secret_file_that_cannot_be_read_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("secret")).unwrap();
        assert!(load_secret(None, dir.path()).is_err());
    }

    #[test]
    fn signing_key_is_stable_and_differs_from_the_root_secret() {
        assert_eq!(signing_key("root"), signing_key("root"));
        assert_ne!(signing_key("root"), signing_key("other"));
        assert_ne!(signing_key("root"), "root");
        assert_eq!(signing_key("root").len(), 64);
    }
}
