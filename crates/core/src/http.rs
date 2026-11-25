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
use reqwest::{Client, ClientBuilder, Method, Proxy as ReqwestProxy, RequestBuilder, Url};

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

/// [`check_url`] for the text a user typed.
pub fn check_url_text(text: &str) -> Result<(), UrlError> {
    check_url(&Url::parse(text).map_err(|_| UrlError::Invalid)?)
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

/// True for a host that is on the local network: a private, loopback or
/// link-local address, `localhost`, a name without a dot (a container name)
/// and the usual local suffixes.
fn is_local(host: url::Host<&str>) -> bool {
    match host {
        url::Host::Ipv4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        url::Host::Ipv6(ip) => {
            ip.is_loopback() || ip.is_unicast_link_local() || (ip.segments()[0] & 0xfe00) == 0xfc00
        }
        url::Host::Domain(name) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            !name.contains('.')
                || [".local", ".lan", ".internal", ".home.arpa"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
        }
    }
}

/// Why a proxy URL was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum ProxyError {
    Invalid,
    Scheme,
    Address,
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "not a valid proxy URL",
            Self::Scheme => "the scheme must be http, https, socks5 or socks5h",
            Self::Address => "this address is not allowed",
        })
    }
}

impl Error for ProxyError {}

/// A proxy for the traffic of indexers (docs/08-integrations.md).
#[derive(Clone)]
pub struct Proxy {
    url: Url,
    bypass_local: bool,
}

impl Proxy {
    /// `http://host:port`, `https://...`, `socks5://...` or `socks5h://...`,
    /// with optional `user:password@`. SOCKS5 always lets the proxy resolve
    /// names, so the local resolver does not see the indexer's name.
    pub fn parse(text: &str, bypass_local: bool) -> Result<Self, ProxyError> {
        let mut url = Url::parse(text.trim()).map_err(|_| ProxyError::Invalid)?;
        match url.scheme() {
            "http" | "https" | "socks5h" => {}
            "socks5" => url
                .set_scheme("socks5h")
                .map_err(|()| ProxyError::Invalid)?,
            _ => return Err(ProxyError::Scheme),
        }
        match url.host() {
            None => return Err(ProxyError::Invalid),
            Some(url::Host::Ipv4(ip)) if is_blocked(ip.into()) => return Err(ProxyError::Address),
            Some(url::Host::Ipv6(ip)) if is_blocked(ip.into()) => return Err(ProxyError::Address),
            Some(_) => {}
        }
        Ok(Self { url, bypass_local })
    }

    /// The address without credentials, for display: `socks5h://host:1080`.
    pub fn display(&self) -> String {
        let mut url = self.url.clone();
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.as_str().trim_end_matches('/').to_owned()
    }

    pub fn has_credentials(&self) -> bool {
        !self.url.username().is_empty() || self.url.password().is_some()
    }

    fn to_reqwest(&self) -> ReqwestProxy {
        let proxy = self.url.clone();
        let bypass_local = self.bypass_local;
        ReqwestProxy::custom(move |target| {
            if bypass_local && target.host().is_some_and(is_local) {
                None
            } else {
                Some(proxy.clone())
            }
        })
    }
}

/// Never shows the credentials.
impl fmt::Debug for Proxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Proxy({})", self.display())
    }
}

/// A `reqwest` client that applies the outbound URL check to every request.
#[derive(Clone)]
pub struct OutboundClient(Client);

impl OutboundClient {
    /// 10 s to connect, 30 s in total (docs/08-integrations.md).
    pub fn new() -> Result<Self, reqwest::Error> {
        Self::builder().build().map(Self)
    }

    /// The same client, with every request that is not local (when the proxy
    /// bypasses local addresses) sent through `proxy`. The check on the URL of
    /// the request still runs; a name is resolved by the proxy, so the
    /// address check on resolved names does not apply to the target.
    pub fn with_proxy(proxy: &Proxy) -> Result<Self, reqwest::Error> {
        // No system proxy on top: the only proxy is the one in settings.
        Self::builder()
            .no_proxy()
            .proxy(proxy.to_reqwest())
            .build()
            .map(Self)
    }

    fn builder() -> ClientBuilder {
        Client::builder()
            .dns_resolver(Arc::new(GuardedResolver))
            .redirect(redirect_policy())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
    }

    /// [`Self::request`] with `GET`.
    pub fn get(&self, url: &str) -> Result<RequestBuilder, UrlError> {
        self.request(Method::GET, url)
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

    #[test]
    fn local_hosts_are_recognised() {
        for local in [
            "http://localhost/",
            "http://prowlarr:9696/",
            "http://192.168.1.10/",
            "http://10.0.0.5/",
            "http://172.16.0.1/",
            "http://127.0.0.1/",
            "http://[::1]/",
            "http://[fd00::1]/",
            "http://nas.local/",
            "http://box.lan/",
        ] {
            assert!(is_local(url(local).host().unwrap()), "{local}");
        }
        for public in [
            "http://indexer.example/",
            "http://8.8.8.8/",
            "http://[2001:db8::1]/",
        ] {
            assert!(!is_local(url(public).host().unwrap()), "{public}");
        }
    }

    #[test]
    fn a_proxy_url_is_checked_and_shown_without_credentials() {
        let socks = Proxy::parse("socks5://user:secret@proxy.example:1080", true).unwrap();
        assert_eq!(socks.display(), "socks5h://proxy.example:1080");
        assert!(socks.has_credentials());
        assert!(!format!("{socks:?}").contains("secret"));
        assert!(
            !Proxy::parse("http://proxy:3128", true)
                .unwrap()
                .has_credentials()
        );

        assert_eq!(
            Proxy::parse("ftp://proxy", true).unwrap_err(),
            ProxyError::Scheme
        );
        assert_eq!(
            Proxy::parse("proxy", true).unwrap_err(),
            ProxyError::Invalid
        );
        assert_eq!(
            Proxy::parse("http://169.254.169.254:3128", true).unwrap_err(),
            ProxyError::Address
        );
    }

    /// A proxy that records the first line of each request and answers 200.
    async fn recording_proxy() -> (SocketAddr, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buffer = [0; 2048];
                    let read = socket.read(&mut buffer).await.unwrap_or(0);
                    let text = String::from_utf8_lossy(&buffer[..read]);
                    log.lock()
                        .unwrap()
                        .push(text.lines().next().unwrap_or_default().to_owned());
                    let _ = socket
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nproxied")
                        .await;
                });
            }
        });
        (addr, seen)
    }

    #[tokio::test]
    async fn a_proxied_request_goes_to_the_proxy() {
        let (proxy, seen) = recording_proxy().await;
        let proxy = Proxy::parse(&format!("http://{proxy}"), true).unwrap();
        let client = OutboundClient::with_proxy(&proxy).unwrap();
        // The name does not resolve: only the proxy can answer.
        let body = client
            .request(Method::GET, "http://indexer.example/api?t=caps")
            .unwrap()
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body, "proxied");
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            ["GET http://indexer.example/api?t=caps HTTP/1.1"]
        );
    }

    #[tokio::test]
    async fn a_local_address_bypasses_the_proxy_only_when_asked() {
        let (proxy_addr, seen) = recording_proxy().await;
        let (target, _) = recording_proxy().await;
        let local = format!("http://127.0.0.1:{}/", target.port());

        let bypass = Proxy::parse(&format!("http://{proxy_addr}"), true).unwrap();
        let client = OutboundClient::with_proxy(&bypass).unwrap();
        client
            .request(Method::GET, &local)
            .unwrap()
            .send()
            .await
            .unwrap();
        assert!(seen.lock().unwrap().is_empty(), "went through the proxy");

        let through = Proxy::parse(&format!("http://{proxy_addr}"), false).unwrap();
        let client = OutboundClient::with_proxy(&through).unwrap();
        client
            .request(Method::GET, &local)
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
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
