//! The qBittorrent WebUI API v2 client (docs/08-integrations.md). This
//! module holds the session: the login and the version call that the test
//! button uses. Later issues add the torrent calls.
//!
//! qBittorrent answers a login with HTTP 200 and the body `Ok.` or `Fails.`,
//! so the status code alone does not tell a wrong password from a good one.

use std::fmt;

use reqwest::header::{COOKIE, REFERER, SET_COOKIE};
use reqwest::{Method, StatusCode};

use crate::http::{OutboundClient, UrlError};

#[derive(Debug, PartialEq, Eq)]
pub enum QbError {
    /// The URL failed the outbound check.
    Url(UrlError),
    /// qBittorrent answered, and refused the username or password.
    Credentials,
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

pub struct QbClient {
    http: OutboundClient,
    base_url: String,
    username: String,
    password: String,
}

// No derive: the password must not reach a log line.
impl fmt::Debug for QbClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QbClient")
            .field("base_url", &self.base_url)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

fn unavailable(error: reqwest::Error) -> QbError {
    // `without_url`: the reason must not echo the address with credentials.
    QbError::Unavailable(error.without_url().to_string())
}

impl QbClient {
    /// `base_url` is the WebUI address, for example `http://qbittorrent:8080`.
    pub fn new(http: OutboundClient, base_url: &str, username: &str, password: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            username: username.to_owned(),
            password: password.to_owned(),
        }
    }

    /// Logs in and returns the session cookie (`SID=...`).
    async fn login(&self) -> Result<String, QbError> {
        let response = self
            .http
            .request(
                Method::POST,
                &format!("{}/api/v2/auth/login", self.base_url),
            )?
            // qBittorrent rejects a request whose Referer is not its own host.
            .header(REFERER, &self.base_url)
            .form(&[
                ("username", self.username.as_str()),
                ("password", self.password.as_str()),
            ])
            .send()
            .await
            .map_err(unavailable)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(QbError::Banned),
            status => return Err(QbError::Unavailable(format!("HTTP {status}"))),
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
        match (body.trim(), cookie) {
            ("Fails.", _) => Err(QbError::Credentials),
            // With "bypass authentication for localhost" there is no cookie.
            ("Ok.", cookie) => Ok(cookie.unwrap_or_default()),
            _ => Err(QbError::Unavailable(
                "the address does not answer like the qBittorrent WebUI".to_owned(),
            )),
        }
    }

    /// Logs in and returns the qBittorrent version, for example `v4.6.2`.
    pub async fn test(&self) -> Result<String, QbError> {
        let cookie = self.login().await?;
        let response = self
            .http
            .request(
                Method::GET,
                &format!("{}/api/v2/app/version", self.base_url),
            )?
            .header(REFERER, &self.base_url)
            .header(COOKIE, cookie)
            .send()
            .await
            .map_err(unavailable)?;
        match response.status() {
            StatusCode::OK => Ok(response
                .text()
                .await
                .map_err(unavailable)?
                .trim()
                .to_owned()),
            StatusCode::FORBIDDEN => Err(QbError::Credentials),
            status => Err(QbError::Unavailable(format!("HTTP {status}"))),
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
            "admin",
            password,
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
            "u",
            "p",
        );
        assert_eq!(client.test().await, Err(QbError::Url(UrlError::Address)));
    }

    #[test]
    fn debug_hides_the_password() {
        let client = QbClient::new(
            OutboundClient::new().unwrap(),
            "http://q",
            "admin",
            "hunter2",
        );
        assert!(!format!("{client:?}").contains("hunter2"));
    }
}
