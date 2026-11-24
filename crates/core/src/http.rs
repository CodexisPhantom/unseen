//! The one HTTP client for every user-entered URL (docs/11-security.md,
//! "Outbound URL check").
//!
//! Private ranges stay open on purpose: indexers and download clients are
//! usually on the local network. Link-local, unspecified, broadcast and
//! multicast addresses are refused, which closes the cloud metadata address.

use std::error::Error;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;
use reqwest::{Client, Method, RequestBuilder, Url};

const MAX_REDIRECTS: usize = 5;

/// Why a URL was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum UrlError {
    Invalid,
    Scheme,
    Address,
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "not a valid URL",
            Self::Scheme => "only http and https URLs are allowed",
            Self::Address => "this address is not allowed",
        })
    }
}

impl Error for UrlError {}

/// True for the addresses the server never calls.
pub fn is_blocked(ip: IpAddr) -> bool {
    // `::ffff:169.254.169.254` is the metadata address in IPv6 clothing.
    match ip.to_canonical() {
        IpAddr::V4(ip) => {
            ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast()
        }
        IpAddr::V6(ip) => ip.is_unicast_link_local() || ip.is_unspecified() || ip.is_multicast(),
    }
}

/// Checks the text of a URL: scheme and, for a literal IP, the address. A
/// host name is checked after resolution, in [`GuardedResolver`].
pub fn check_url(url: &Url) -> Result<(), UrlError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(UrlError::Scheme);
    }
    match url.host() {
        None => Err(UrlError::Invalid),
        Some(url::Host::Ipv4(ip)) if is_blocked(ip.into()) => Err(UrlError::Address),
        Some(url::Host::Ipv6(ip)) if is_blocked(ip.into()) => Err(UrlError::Address),
        Some(_) => Ok(()),
    }
}

/// Resolves a name with the system resolver and drops blocked addresses, so
/// a DNS name cannot point at one. Nothing left means the connection fails.
struct GuardedResolver;

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let found = tokio::net::lookup_host((name.as_str(), 0)).await?;
            let allowed = allowed_addresses(found)?;
            Ok(Box::new(allowed.into_iter()) as Addrs)
        })
    }
}

fn allowed_addresses(found: impl Iterator<Item = SocketAddr>) -> Result<Vec<SocketAddr>, UrlError> {
    let allowed: Vec<_> = found.filter(|a| !is_blocked(a.ip())).collect();
    if allowed.is_empty() {
        Err(UrlError::Address)
    } else {
        Ok(allowed)
    }
}

/// Follows a redirect only to the same host and at most [`MAX_REDIRECTS`]
/// times.
fn redirect_policy() -> Policy {
    Policy::custom(|attempt| {
        let same_host = attempt
            .previous()
            .last()
            .is_some_and(|from| from.host() == attempt.url().host());
        if attempt.previous().len() >= MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if !same_host || check_url(attempt.url()).is_err() {
            attempt.stop()
        } else {
            attempt.follow()
        }
    })
}

/// A `reqwest` client that applies the outbound URL check to every request.
#[derive(Clone)]
pub struct OutboundClient(Client);

impl OutboundClient {
    /// 10 s to connect, 30 s in total (docs/08-integrations.md).
    pub fn new() -> Result<Self, reqwest::Error> {
        Client::builder()
            .dns_resolver(Arc::new(GuardedResolver))
            .redirect(redirect_policy())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map(Self)
    }

    /// Starts a request, or refuses the URL before anything is sent.
    pub fn request(&self, method: Method, url: &str) -> Result<RequestBuilder, UrlError> {
        let url = Url::parse(url).map_err(|_| UrlError::Invalid)?;
        check_url(&url)?;
        Ok(self.0.request(method, url))
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn refuses_metadata_and_other_blocked_literals() {
        for blocked in [
            "http://169.254.169.254/",
            "http://[fe80::1]/",
            "http://[::ffff:169.254.169.254]/",
            "http://0.0.0.0/",
            "http://255.255.255.255/",
            "http://224.0.0.1/",
            "http://[ff02::1]/",
            "http://[::]/",
        ] {
            assert_eq!(
                check_url(&url(blocked)),
                Err(UrlError::Address),
                "{blocked}"
            );
        }
    }

    #[test]
    fn allows_local_network_and_names() {
        for allowed in [
            "http://192.168.1.10:8080/",
            "http://10.0.0.5/",
            "http://127.0.0.1:9117/",
            "http://localhost:9117/",
            "https://indexer.example/",
        ] {
            assert_eq!(check_url(&url(allowed)), Ok(()), "{allowed}");
        }
    }

    #[test]
    fn allows_http_and_https_only() {
        assert_eq!(check_url(&url("ftp://example.com/")), Err(UrlError::Scheme));
        assert_eq!(check_url(&url("file:///etc/passwd")), Err(UrlError::Scheme));
    }

    #[tokio::test]
    async fn request_is_refused_before_it_is_sent() {
        let client = OutboundClient::new().unwrap();
        assert!(
            client
                .request(Method::GET, "http://169.254.169.254/")
                .is_err()
        );
        assert!(client.request(Method::GET, "nonsense").is_err());
    }

    #[tokio::test]
    async fn resolver_drops_blocked_addresses() {
        // `localhost` resolves to loopback only, which is allowed.
        let name: Name = "localhost".parse().unwrap();
        let addrs: Vec<_> = GuardedResolver.resolve(name).await.unwrap().collect();
        assert!(!addrs.is_empty() && addrs.iter().all(|a| !is_blocked(a.ip())));
    }

    #[test]
    fn a_name_that_resolves_to_a_blocked_address_is_refused() {
        let metadata: SocketAddr = "169.254.169.254:0".parse().unwrap();
        let lan: SocketAddr = "192.168.1.10:0".parse().unwrap();
        assert_eq!(
            allowed_addresses([metadata].into_iter()),
            Err(UrlError::Address)
        );
        assert_eq!(
            allowed_addresses([metadata, lan].into_iter()),
            Ok(vec![lan])
        );
    }

    /// Answers every connection with `response` and returns its address.
    async fn serve(response: String) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let response = response.clone();
                tokio::spawn(async move {
                    let mut buffer = [0; 1024];
                    let _ = socket.read(&mut buffer).await;
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn reaches_localhost_and_does_not_follow_a_redirect_to_another_host() {
        let client = OutboundClient::new().unwrap();

        let ok = serve("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".into()).await;
        let response = client
            .request(Method::GET, &format!("http://localhost:{}/", ok.port()))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);

        // The redirect points at the metadata address: it is not followed.
        let redirect = serve(
            "HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/\r\nContent-Length: 0\r\n\r\n"
                .into(),
        )
        .await;
        let response = client
            .request(
                Method::GET,
                &format!("http://localhost:{}/", redirect.port()),
            )
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 302);
    }

    #[tokio::test]
    async fn follows_a_redirect_on_the_same_host_and_stops_after_five() {
        let client = OutboundClient::new().unwrap();

        // The server redirects to itself forever.
        let looping = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = looping.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = looping.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buffer = [0; 1024];
                    let _ = socket.read(&mut buffer).await;
                    let reply = format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/\r\nContent-Length: 0\r\n\r\n"
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        let result = client
            .request(Method::GET, &format!("http://127.0.0.1:{port}/"))
            .unwrap()
            .send()
            .await;
        assert!(result.unwrap_err().is_redirect());
    }
}
