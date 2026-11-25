//! The qBittorrent WebUI API v2 client (docs/08-integrations.md). This
//! module holds the session: the login and the version call that the test
//! button uses. Later issues add the torrent calls.
//!
//! Two ways to authenticate. With a username and a password the client logs
//! in and keeps the session cookie. qBittorrent answers a login with HTTP 200
//! and the body `Ok.` or `Fails.`, so the status code alone does not tell a
//! wrong password from a good one. With an API key (qBittorrent 5.2 or later)
//! every request carries `Authorization: Bearer <key>` and there is no login:
//! a key cannot call the auth endpoints.

use std::fmt;

use reqwest::header::{AUTHORIZATION, COOKIE, REFERER, SET_COOKIE};
use reqwest::{Method, StatusCode};

use crate::http::{OutboundClient, UrlError};

#[derive(Debug, PartialEq, Eq)]
pub enum QbError {
    /// The URL failed the outbound check.
    Url(UrlError),
    /// qBittorrent answered, and refused the username or password.
    Credentials,
    /// qBittorrent answered, and refused the API key.
    ApiKey,
    /// qBittorrent banned this address after failed logins (HTTP 403).
    Banned,
    /// No answer, or an answer that is not from the WebUI API.
    Unavailable(String),
}

impl fmt::Display for QbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(error) => write!(f, "{error}"),
            Self::Credentials => f.write_str("qBittorrent refused the username or password"),
            Self::ApiKey => f.write_str("qBittorrent refused the API key"),
            Self::Banned => f.write_str("qBittorrent banned this address after failed logins"),
            Self::Unavailable(reason) => write!(f, "qBittorrent is not available: {reason}"),
        }
    }
}

impl std::error::Error for QbError {}

impl From<UrlError> for QbError {
    fn from(error: UrlError) -> Self {
        Self::Url(error)
    }
}

/// How the client proves who it is.
pub enum QbAuth {
    /// Log in with these and use the session cookie. Both may be empty when
    /// qBittorrent bypasses authentication for the address of this server.
    Password { username: String, password: String },
    /// An API key of qBittorrent 5.2 or later (`qbt_...`).
    ApiKey(String),
}

pub struct QbClient {
    http: OutboundClient,
    base_url: String,
    auth: QbAuth,
}

// No derive: the password and the key must not reach a log line.
impl fmt::Debug for QbClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QbClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

/// The step of the test, so an error says where it happened. The body is the
/// first words of a text answer, which tells a qBittorrent message from a
/// proxy page.
async fn unexpected(step: &str, response: reqwest::Response) -> QbError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let body: String = body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect();
    QbError::Unavailable(if body.is_empty() {
        format!("{step} answered HTTP {status}")
    } else {
        format!("{step} answered HTTP {status}: {body}")
    })
}

fn unavailable(error: reqwest::Error) -> QbError {
    // `without_url`: the reason must not echo the address with credentials.
    QbError::Unavailable(error.without_url().to_string())
}

impl QbClient {
    /// `base_url` is the WebUI address, for example `http://qbittorrent:8080`.
    pub fn new(http: OutboundClient, base_url: &str, auth: QbAuth) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            auth,
        }
    }

    /// Logs in and returns the session cookie (`SID=...`).
    async fn login(&self, username: &str, password: &str) -> Result<String, QbError> {
        let response = self
            .http
            .request(
                Method::POST,
                &format!("{}/api/v2/auth/login", self.base_url),
            )?
            // qBittorrent rejects a request whose Referer is not its own host.
            .header(REFERER, &self.base_url)
            .form(&[("username", username), ("password", password)])
            .send()
            .await
            .map_err(unavailable)?;
        let status = response.status();
        match status {
            // Older versions answer 200 with a body; newer ones may answer
            // 204 without one.
            StatusCode::OK | StatusCode::NO_CONTENT => {}
            StatusCode::UNAUTHORIZED => return Err(QbError::Credentials),
            StatusCode::FORBIDDEN => return Err(QbError::Banned),
            _ => return Err(unexpected("the login", response).await),
        }
        let cookie = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|cookie| cookie.starts_with("SID="))
            .and_then(|cookie| cookie.split(';').next())
            .map(str::to_owned);
        let body = response.text().await.map_err(unavailable)?;
        if status == StatusCode::NO_CONTENT {
            return Ok(cookie.unwrap_or_default());
        }
        match (body.trim(), cookie) {
            ("Fails.", _) => Err(QbError::Credentials),
            // With "bypass authentication for localhost" there is no cookie.
            ("Ok.", cookie) => Ok(cookie.unwrap_or_default()),
            _ => Err(QbError::Unavailable(
                "the address does not answer like the qBittorrent WebUI".to_owned(),
            )),
        }
    }

    /// Authenticates and returns the qBittorrent version, for example
    /// `v4.6.2`.
    pub async fn test(&self) -> Result<String, QbError> {
        let request = self.http.request(
            Method::GET,
            &format!("{}/api/v2/app/version", self.base_url),
        )?;
        let request = match &self.auth {
            QbAuth::ApiKey(key) => request.header(AUTHORIZATION, format!("Bearer {key}")),
            QbAuth::Password { username, password } => {
                let cookie = self.login(username, password).await?;
                // No cookie: qBittorrent let this address in without a login.
                if cookie.is_empty() {
                    request
                } else {
                    request.header(COOKIE, cookie)
                }
            }
        };
        let response = request
            .header(REFERER, &self.base_url)
            .send()
            .await
            .map_err(unavailable)?;
        match (response.status(), &self.auth) {
            (StatusCode::OK, _) => Ok(response
                .text()
                .await
                .map_err(unavailable)?
                .trim()
                .to_owned()),
            (StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN, QbAuth::ApiKey(_)) => {
                Err(QbError::ApiKey)
            }
            (StatusCode::FORBIDDEN, QbAuth::Password { .. }) => Err(QbError::Credentials),
            _ => Err(unexpected("the version call", response).await),
        }
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn client(server: &MockServer, password: &str) -> QbClient {
        QbClient::new(
            OutboundClient::new().unwrap(),
            &format!("{}/", server.uri()),
            QbAuth::Password {
                username: "admin".into(),
                password: password.into(),
            },
        )
    }

    fn key_client(server: &MockServer, key: &str) -> QbClient {
        QbClient::new(
            OutboundClient::new().unwrap(),
            &server.uri(),
            QbAuth::ApiKey(key.into()),
        )
    }

    async fn login_answer(server: &MockServer, response: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .respond_with(response)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_good_login_returns_the_version() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .and(body_string_contains("username=admin"))
            .and(body_string_contains("password=pa%26ss"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", "SID=abc123; HttpOnly; path=/")
                    .set_body_string("Ok."),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .and(header("cookie", "SID=abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_string("v4.6.2"))
            .mount(&server)
            .await;
        assert_eq!(
            client(&server, "pa&ss").test().await.as_deref(),
            Ok("v4.6.2")
        );
    }

    #[tokio::test]
    async fn a_200_with_fails_is_wrong_credentials() {
        let server = MockServer::start().await;
        login_answer(
            &server,
            ResponseTemplate::new(200).set_body_string("Fails."),
        )
        .await;
        assert_eq!(
            client(&server, "bad").test().await,
            Err(QbError::Credentials)
        );
    }

    #[tokio::test]
    async fn a_204_login_is_a_good_login_and_a_401_is_wrong_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .respond_with(ResponseTemplate::new(204).insert_header("set-cookie", "SID=n1; path=/"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .and(header("cookie", "SID=n1"))
            .respond_with(ResponseTemplate::new(200).set_body_string("v5.1.0"))
            .mount(&server)
            .await;
        assert_eq!(
            client(&server, "good").test().await.as_deref(),
            Ok("v5.1.0")
        );

        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(401)).await;
        assert_eq!(
            client(&server, "bad").test().await,
            Err(QbError::Credentials)
        );
    }

    #[tokio::test]
    async fn a_403_on_login_is_a_ban() {
        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(403)).await;
        assert_eq!(client(&server, "x").test().await, Err(QbError::Banned));
    }

    #[tokio::test]
    async fn another_server_is_not_qbittorrent() {
        let server = MockServer::start().await;
        login_answer(
            &server,
            ResponseTemplate::new(200).set_body_string("<html>"),
        )
        .await;
        assert!(matches!(
            client(&server, "x").test().await,
            Err(QbError::Unavailable(_))
        ));
        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(404)).await;
        assert!(matches!(
            client(&server, "x").test().await,
            Err(QbError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn a_login_without_cookie_works_for_localhost_bypass() {
        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(200).set_body_string("Ok.")).await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .respond_with(ResponseTemplate::new(200).set_body_string("v5.0.0"))
            .mount(&server)
            .await;
        assert_eq!(client(&server, "").test().await.as_deref(), Ok("v5.0.0"));
    }

    #[tokio::test]
    async fn a_refused_address_is_not_called() {
        let client = QbClient::new(
            OutboundClient::new().unwrap(),
            "http://169.254.169.254",
            QbAuth::Password {
                username: "u".into(),
                password: "p".into(),
            },
        );
        assert_eq!(client.test().await, Err(QbError::Url(UrlError::Address)));
    }

    #[test]
    fn debug_hides_the_password_and_the_key() {
        let client = QbClient::new(
            OutboundClient::new().unwrap(),
            "http://q",
            QbAuth::Password {
                username: "admin".into(),
                password: "hunter2".into(),
            },
        );
        assert!(!format!("{client:?}").contains("hunter2"));
        let client = QbClient::new(
            OutboundClient::new().unwrap(),
            "http://q",
            QbAuth::ApiKey("qbt_secretsecretsecretsecret1234".into()),
        );
        assert!(!format!("{client:?}").contains("qbt_"));
    }

    #[tokio::test]
    async fn an_api_key_goes_in_a_bearer_header_and_no_login_is_made() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v2/auth/login"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .and(header("authorization", "Bearer qbt_abc"))
            .respond_with(ResponseTemplate::new(200).set_body_string("v5.2.0"))
            .mount(&server)
            .await;
        assert_eq!(
            key_client(&server, "qbt_abc").test().await.as_deref(),
            Ok("v5.2.0")
        );
    }

    #[tokio::test]
    async fn a_refused_api_key_is_named() {
        for status in [401, 403] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/api/v2/app/version"))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;
            assert_eq!(
                key_client(&server, "qbt_bad").test().await,
                Err(QbError::ApiKey),
                "{status}"
            );
        }
    }

    #[tokio::test]
    async fn an_unexpected_answer_names_the_step_status_and_body() {
        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(200).set_body_string("Ok.")).await;
        Mock::given(method("GET"))
            .and(path("/api/v2/app/version"))
            .respond_with(ResponseTemplate::new(404).set_body_string(
                "Not
Found",
            ))
            .mount(&server)
            .await;
        let Err(QbError::Unavailable(reason)) = client(&server, "x").test().await else {
            panic!("expected an unavailable error");
        };
        assert_eq!(
            reason,
            "the version call answered HTTP 404 Not Found: Not Found"
        );

        let server = MockServer::start().await;
        login_answer(&server, ResponseTemplate::new(500)).await;
        let Err(QbError::Unavailable(reason)) = client(&server, "x").test().await else {
            panic!("expected an unavailable error");
        };
        assert_eq!(reason, "the login answered HTTP 500 Internal Server Error");
    }
}
