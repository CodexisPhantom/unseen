use std::time::Duration;

use wiremock::matchers::{method, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

const JACKETT_CAPS: &str = include_str!("../../../../fixtures/torznab/jackett_caps.xml");
const JACKETT_MOVIE: &str = include_str!("../../../../fixtures/torznab/jackett_movie.xml");
const PROWLARR_CAPS: &str = include_str!("../../../../fixtures/torznab/prowlarr_caps_no_imdb.xml");
const PROWLARR_MOVIE: &str = include_str!("../../../../fixtures/torznab/prowlarr_movie.xml");
const ERROR_CREDENTIALS: &str = include_str!("../../../../fixtures/torznab/error_credentials.xml");
const ERROR_FUNCTION: &str = include_str!("../../../../fixtures/torznab/error_function.xml");

fn query() -> MovieQuery {
    MovieQuery {
        tmdb_id: Some(603),
        imdb_id: Some("tt0133093".to_owned()),
        title: "The Matrix".to_owned(),
        year: Some(1999),
        categories: vec![2000, 2040],
    }
}

fn pairs(params: Params) -> Vec<(String, String)> {
    params.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

fn expect(params: Params, expected: &[(&str, &str)]) {
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    assert_eq!(pairs(params), expected);
}

#[test]
fn a_jackett_caps_answer_lists_modes_and_categories() {
    let caps = parse_caps(JACKETT_CAPS).unwrap();
    let movie = caps.movie_search.unwrap();
    assert!(movie.supports("imdbid") && movie.supports("tmdbid") && movie.supports("q"));
    assert_eq!(caps.search.unwrap().params, ["q"]);
    assert!(
        caps.categories
            .iter()
            .any(|c| c.id == 2000 && c.name == "Movies")
    );
    assert!(caps.categories.iter().any(|c| c.id == 2045));
}

#[test]
fn a_mode_marked_unavailable_is_absent() {
    let xml = r#"<caps><searching>
        <search available="yes" supportedParams="q"/>
        <movie-search available="no" supportedParams="q,imdbid"/>
    </searching></caps>"#;
    let caps = parse_caps(xml).unwrap();
    assert!(caps.movie_search.is_none());
    assert!(caps.search.is_some());
}

#[test]
fn an_imdb_id_wins_and_loses_its_prefix() {
    let caps = parse_caps(JACKETT_CAPS).unwrap();
    let params = movie_params(&caps, &query()).unwrap();
    expect(
        params,
        &[("t", "movie"), ("imdbid", "0133093"), ("cat", "2000,2040")],
    );
}

#[test]
fn a_tmdb_id_is_used_when_there_is_no_imdb_id() {
    let caps = parse_caps(JACKETT_CAPS).unwrap();
    let query = MovieQuery {
        imdb_id: None,
        ..query()
    };
    expect(
        movie_params(&caps, &query).unwrap(),
        &[("t", "movie"), ("tmdbid", "603"), ("cat", "2000,2040")],
    );
}

#[test]
fn caps_without_imdbid_fall_back_to_a_text_search() {
    let caps = parse_caps(PROWLARR_CAPS).unwrap();
    expect(
        movie_params(&caps, &query()).unwrap(),
        &[("t", "movie"), ("q", "The Matrix"), ("cat", "2000,2040")],
    );
}

#[test]
fn a_text_search_sends_the_year_only_when_it_is_supported() {
    let xml = r#"<caps><searching>
        <movie-search available="yes" supportedParams="q,year"/>
    </searching></caps>"#;
    let caps = parse_caps(xml).unwrap();
    expect(
        movie_params(
            &caps,
            &MovieQuery {
                categories: vec![],
                ..query()
            },
        )
        .unwrap(),
        &[("t", "movie"), ("q", "The Matrix"), ("year", "1999")],
    );
}

#[test]
fn an_indexer_without_a_movie_mode_gets_a_plain_search_with_the_year_in_the_text() {
    let xml =
        r#"<caps><searching><search available="yes" supportedParams="q"/></searching></caps>"#;
    let caps = parse_caps(xml).unwrap();
    expect(
        movie_params(
            &caps,
            &MovieQuery {
                categories: vec![],
                ..query()
            },
        )
        .unwrap(),
        &[("t", "search"), ("q", "The Matrix 1999")],
    );
}

#[test]
fn an_indexer_that_cannot_search_for_the_movie_is_an_error() {
    let caps = parse_caps("<caps><searching/></caps>").unwrap();
    assert!(matches!(
        movie_params(&caps, &query()),
        Err(TorznabError::Unavailable(_))
    ));
    // Id-only movie search and no id known.
    let xml = r#"<caps><searching>
        <movie-search available="yes" supportedParams="imdbid"/>
    </searching></caps>"#;
    let caps = parse_caps(xml).unwrap();
    let query = MovieQuery {
        imdb_id: None,
        ..query()
    };
    assert!(movie_params(&caps, &query).is_err());
}

#[test]
fn the_feed_uses_the_movie_mode_else_the_plain_search() {
    let caps = parse_caps(JACKETT_CAPS).unwrap();
    expect(
        feed_params(&caps, &[2000]).unwrap(),
        &[("t", "movie"), ("cat", "2000")],
    );
    let xml =
        r#"<caps><searching><search available="yes" supportedParams="q"/></searching></caps>"#;
    expect(
        feed_params(&parse_caps(xml).unwrap(), &[]).unwrap(),
        &[("t", "search")],
    );
    assert!(feed_params(&Caps::default(), &[]).is_err());
}

#[test]
fn a_jackett_feed_parses_every_field() {
    let releases = parse_feed(JACKETT_MOVIE).unwrap();
    // The third item has no link and is skipped.
    assert_eq!(releases.len(), 2);
    let first = &releases[0];
    assert_eq!(first.title, "The.Matrix.1999.1080p.BluRay.x264-GRP");
    assert_eq!(first.guid, "https://tracker.example/details/1001");
    assert_eq!(first.size, Some(8_589_934_592));
    assert_eq!(first.seeders, Some(42));
    assert_eq!(first.peers, Some(45));
    assert_eq!(
        first.info_hash.as_deref(),
        Some("c9e15763f722f23e98a29decdfae341b98d53056")
    );
    assert!(
        first
            .download_url
            .as_deref()
            .unwrap()
            .contains("/dl/exampletracker/")
    );
    assert!(
        first
            .magnet_url
            .as_deref()
            .unwrap()
            .starts_with("magnet:?xt=urn:btih:")
    );
    assert_eq!(first.categories, [2000, 2040]);
    assert_eq!(
        first.published.unwrap().to_rfc3339(),
        "2026-11-22T18:30:00+00:00"
    );
}

#[test]
fn a_magnet_in_the_link_element_becomes_the_magnet_url() {
    let second = &parse_feed(JACKETT_MOVIE).unwrap()[1];
    assert_eq!(second.download_url, None);
    assert!(
        second
            .magnet_url
            .as_deref()
            .unwrap()
            .starts_with("magnet:?xt=urn:btih:0123")
    );
    assert_eq!(second.seeders, Some(0));
    assert_eq!(second.size, Some(21_474_836_480));
    assert_eq!(second.categories, [2045]);
    assert_eq!(second.info_hash, None);
}

#[test]
fn a_prowlarr_feed_parses_with_gmt_dates_and_size_attributes() {
    let releases = parse_feed(PROWLARR_MOVIE).unwrap();
    assert_eq!(releases.len(), 1);
    let release = &releases[0];
    assert_eq!(release.title, "Inception 2010 1080p BluRay DTS x264-GRP");
    assert_eq!(release.size, Some(12_884_901_888));
    assert_eq!(release.seeders, Some(120));
    assert_eq!(release.peers, Some(150));
    assert_eq!(
        release.info_hash.as_deref(),
        Some("aabbccddeeff00112233445566778899aabbccdd")
    );
    assert_eq!(release.magnet_url, None);
    assert_eq!(
        release.published.unwrap().to_rfc3339(),
        "2026-11-17T12:00:00+00:00"
    );
}

#[test]
fn error_elements_are_named() {
    assert_eq!(parse_feed(ERROR_CREDENTIALS), Err(TorznabError::ApiKey));
    assert_eq!(parse_caps(ERROR_CREDENTIALS), Err(TorznabError::ApiKey));
    assert_eq!(
        parse_feed(ERROR_FUNCTION),
        Err(TorznabError::Indexer {
            code: 203,
            description: "Function Not Available: movie search not supported".to_owned()
        })
    );
}

#[test]
fn an_html_page_or_the_wrong_document_is_not_torznab() {
    for body in [
        "<html><body>Login</body></html>",
        "not xml at all",
        "<rss/>x",
    ] {
        assert!(matches!(
            parse_feed(body),
            Err(TorznabError::Unavailable(_))
        ));
    }
    assert!(matches!(
        parse_caps(JACKETT_MOVIE),
        Err(TorznabError::Unavailable(_))
    ));
}

#[test]
fn a_doctype_is_refused() {
    let xml = r#"<?xml version="1.0"?><!DOCTYPE x [<!ENTITY a "aaaa">]><rss/>"#;
    assert!(matches!(parse_feed(xml), Err(TorznabError::Unavailable(_))));
}

fn client(server: &MockServer, key: &str) -> TorznabClient {
    TorznabClient::new(OutboundClient::new().unwrap(), &server.uri(), key)
}

fn xml(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "application/xml")
        .set_body_string(body)
}

#[tokio::test]
async fn caps_then_an_id_search_send_the_key_and_the_parameters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("t", "caps"))
        .and(query_param("apikey", "secret"))
        .respond_with(xml(JACKETT_CAPS))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .and(query_param("imdbid", "0133093"))
        .and(query_param("cat", "2000,2040"))
        .and(query_param("apikey", "secret"))
        .respond_with(xml(JACKETT_MOVIE))
        .mount(&server)
        .await;
    let client = client(&server, "secret");
    let caps = client.caps().await.unwrap();
    let releases = client.search_movie(&caps, &query()).await.unwrap();
    assert_eq!(releases.len(), 2);
}

#[tokio::test]
async fn an_indexer_without_imdbid_is_searched_by_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .and(query_param("q", "The Matrix"))
        .and(query_param_is_missing("imdbid"))
        .and(query_param_is_missing("tmdbid"))
        .respond_with(xml(PROWLARR_MOVIE))
        .expect(1)
        .mount(&server)
        .await;
    let caps = parse_caps(PROWLARR_CAPS).unwrap();
    let releases = client(&server, "k")
        .search_movie(&caps, &query())
        .await
        .unwrap();
    assert_eq!(releases.len(), 1);
}

#[tokio::test]
async fn the_latest_feed_has_no_query_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .and(query_param_is_missing("q"))
        .respond_with(xml(JACKETT_MOVIE))
        .mount(&server)
        .await;
    let caps = parse_caps(JACKETT_CAPS).unwrap();
    assert_eq!(
        client(&server, "k").latest(&caps, &[]).await.unwrap().len(),
        2
    );
}

#[tokio::test]
async fn a_wrong_key_is_named_on_200_and_on_401() {
    for status in [200, 401, 403] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("content-type", "application/xml")
                    .set_body_string(ERROR_CREDENTIALS),
            )
            .mount(&server)
            .await;
        assert_eq!(
            client(&server, "wrong").caps().await.unwrap_err(),
            TorznabError::ApiKey,
            "{status}"
        );
    }
    // A bare 401 without a body.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    assert_eq!(
        client(&server, "wrong").caps().await.unwrap_err(),
        TorznabError::ApiKey
    );
}

#[tokio::test]
async fn a_429_reports_the_wait_to_back_off() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "120"))
        .mount(&server)
        .await;
    assert_eq!(
        client(&server, "k").caps().await.unwrap_err(),
        TorznabError::RateLimited {
            retry_after: Some(Duration::from_secs(120))
        }
    );
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;
    assert_eq!(
        client(&server, "k").caps().await.unwrap_err(),
        TorznabError::RateLimited { retry_after: None }
    );
}

#[tokio::test]
async fn a_server_error_names_the_status_and_not_the_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(502).set_body_string("<html>Bad gateway</html>"))
        .mount(&server)
        .await;
    let error = client(&server, "topsecret").caps().await.unwrap_err();
    let text = error.to_string();
    assert!(text.contains("502"), "{text}");
    assert!(!text.contains("topsecret"));
}

#[tokio::test]
async fn a_refused_connection_does_not_leak_the_key() {
    let server = MockServer::start().await;
    let client = client(&server, "topsecret");
    drop(server);
    let error = client.caps().await.unwrap_err();
    assert!(matches!(error, TorznabError::Unavailable(_)));
    assert!(!error.to_string().contains("topsecret"));
    assert!(!format!("{client:?}").contains("topsecret"));
}

#[tokio::test]
async fn a_refused_address_is_not_called() {
    let client = TorznabClient::new(
        OutboundClient::new().unwrap(),
        "http://169.254.169.254/",
        "k",
    );
    assert_eq!(
        client.caps().await.unwrap_err(),
        TorznabError::Url(UrlError::Address)
    );
}

#[tokio::test]
async fn the_test_reads_caps_and_asks_for_one_item() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("t", "caps"))
        .respond_with(xml(JACKETT_CAPS))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .and(query_param("limit", "1"))
        .respond_with(xml(JACKETT_MOVIE))
        .expect(1)
        .mount(&server)
        .await;
    let caps = client(&server, "k").test().await.unwrap();
    assert!(caps.movie_search.is_some());
}

#[tokio::test]
async fn the_test_finds_a_wrong_key_that_caps_does_not_check() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("t", "caps"))
        .respond_with(xml(JACKETT_CAPS))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .respond_with(xml(ERROR_CREDENTIALS))
        .mount(&server)
        .await;
    assert_eq!(
        client(&server, "wrong").test().await.unwrap_err(),
        TorznabError::ApiKey
    );
}
