use std::time::Duration;

use chrono::{TimeZone, Utc};
use wiremock::matchers::{method, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

const NZBGEEK_CAPS: &str = include_str!("../../../../fixtures/newznab/nzbgeek_caps.xml");
const NZBGEEK_MOVIE: &str = include_str!("../../../../fixtures/newznab/nzbgeek_movie.xml");
const DRUNKENSLUG_CAPS: &str = include_str!("../../../../fixtures/newznab/drunkenslug_caps.xml");
const DRUNKENSLUG_MOVIE: &str = include_str!("../../../../fixtures/newznab/drunkenslug_movie.xml");
const ERROR_CREDENTIALS: &str = include_str!("../../../../fixtures/newznab/error_credentials.xml");
const ERROR_RATE_LIMIT: &str = include_str!("../../../../fixtures/newznab/error_rate_limit.xml");

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
fn nzbgeek_and_drunkenslug_caps_parse_modes_limits_retention_and_categories() {
    let geek = parse_caps(NZBGEEK_CAPS).unwrap();
    let geek_movie = geek.movie_search.unwrap();
    assert!(geek_movie.supports("imdbid"));
    assert!(geek_movie.supports("tmdbid"));
    assert!(geek_movie.supports("q"));
    assert_eq!(geek.search.unwrap().params, ["q"]);
    assert_eq!(geek.limits_max, Some(100));
    assert_eq!(geek.limits_default, Some(100));
    assert_eq!(geek.retention_days, Some(3500));
    assert!(
        geek.categories
            .iter()
            .any(|c| c.id == 2000 && c.name == "Movies")
    );
    assert!(geek.categories.iter().any(|c| c.id == 2045));

    let slug = parse_caps(DRUNKENSLUG_CAPS).unwrap();
    let slug_movie = slug.movie_search.unwrap();
    assert!(slug_movie.supports("imdbid"));
    assert!(!slug_movie.supports("tmdbid"));
    assert_eq!(slug.limits_max, Some(100));
    assert_eq!(slug.limits_default, Some(50));
    assert_eq!(slug.retention_days, Some(4200));
    assert!(slug.categories.iter().any(|c| c.id == 2050));
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
fn movie_params_prefer_imdb_then_tmdb_then_text_and_respect_caps() {
    let geek = parse_caps(NZBGEEK_CAPS).unwrap();
    expect(
        movie_params(&geek, &query()).unwrap(),
        &[
            ("t", "movie"),
            ("imdbid", "0133093"),
            ("extended", "1"),
            ("cat", "2000,2040"),
        ],
    );

    let no_imdb = MovieQuery {
        imdb_id: None,
        ..query()
    };
    expect(
        movie_params(&geek, &no_imdb).unwrap(),
        &[
            ("t", "movie"),
            ("tmdbid", "603"),
            ("extended", "1"),
            ("cat", "2000,2040"),
        ],
    );

    // DrunkenSlug does not advertise tmdbid, so without imdbid it falls back to movie text search.
    let slug = parse_caps(DRUNKENSLUG_CAPS).unwrap();
    expect(
        movie_params(&slug, &no_imdb).unwrap(),
        &[
            ("t", "movie"),
            ("q", "The Matrix"),
            ("extended", "1"),
            ("cat", "2000,2040"),
        ],
    );
}

#[test]
fn movie_text_search_sends_year_only_when_supported_and_falls_back_to_plain_search() {
    let with_year = parse_caps(
        r#"<caps><searching><movie-search available="yes" supportedParams="q,year"/></searching></caps>"#,
    )
    .unwrap();
    expect(
        movie_params(
            &with_year,
            &MovieQuery {
                categories: vec![],
                ..query()
            },
        )
        .unwrap(),
        &[
            ("t", "movie"),
            ("q", "The Matrix"),
            ("year", "1999"),
            ("extended", "1"),
        ],
    );

    let search_only = parse_caps(
        r#"<caps><searching><search available="yes" supportedParams="q"/></searching></caps>"#,
    )
    .unwrap();
    expect(
        movie_params(
            &search_only,
            &MovieQuery {
                categories: vec![],
                ..query()
            },
        )
        .unwrap(),
        &[("t", "search"), ("q", "The Matrix 1999"), ("extended", "1")],
    );

    expect(
        movie_params(
            &search_only,
            &MovieQuery {
                categories: vec![],
                year: None,
                ..query()
            },
        )
        .unwrap(),
        &[("t", "search"), ("q", "The Matrix"), ("extended", "1")],
    );

    let empty_caps = parse_caps("<caps><searching/></caps>").unwrap();
    assert!(matches!(
        movie_params(&empty_caps, &query()),
        Err(NewznabError::Unavailable(_))
    ));
}

#[test]
fn text_and_rss_params_follow_caps() {
    let geek = parse_caps(NZBGEEK_CAPS).unwrap();
    expect(
        text_params(&geek, " The Matrix 1999 ", &[2000, 2040]).unwrap(),
        &[
            ("t", "search"),
            ("q", "The Matrix 1999"),
            ("extended", "1"),
            ("cat", "2000,2040"),
        ],
    );
    assert!(matches!(
        text_params(&geek, "   ", &[]),
        Err(NewznabError::Unavailable(_))
    ));
    assert!(matches!(
        text_params(&Caps::default(), "Matrix", &[]),
        Err(NewznabError::Unavailable(_))
    ));

    expect(
        rss_params(&geek, &[2000]).unwrap(),
        &[("t", "movie"), ("extended", "1"), ("cat", "2000")],
    );
    let search_only = parse_caps(
        r#"<caps><searching><search available="yes" supportedParams="q"/></searching></caps>"#,
    )
    .unwrap();
    expect(
        rss_params(&search_only, &[]).unwrap(),
        &[("t", "search"), ("extended", "1")],
    );
    assert!(matches!(
        rss_params(&Caps::default(), &[]),
        Err(NewznabError::Unavailable(_))
    ));
}

#[test]
fn nzbgeek_feed_parses_nzb_enclosures_attributes_and_age() {
    let releases = parse_feed(NZBGEEK_MOVIE).unwrap();
    // Third item has no NZB URL or link and is skipped.
    assert_eq!(releases.len(), 2);

    let first = &releases[0];
    assert_eq!(first.title, "The.Matrix.1999.1080p.BluRay.x264-GRP");
    assert_eq!(
        first.guid,
        "https://nzbgeek.info/geekseek.php?guid=a1b2c3d4e5f60001"
    );
    assert_eq!(
        first.download_url,
        "https://api.nzbgeek.info/api?t=get&id=a1b2c3d4e5f60001&apikey=demo"
    );
    assert_eq!(first.size, Some(8_589_934_592));
    assert_eq!(first.categories, [2000, 2040]);
    assert_eq!(first.grabs, Some(184));
    assert_eq!(first.files, Some(92));
    assert_eq!(
        first.poster.as_deref(),
        Some("uploader@usenet.example (MovieBot)")
    );
    assert_eq!(first.group.as_deref(), Some("alt.binaries.movies"));
    assert_eq!(first.imdb_id.as_deref(), Some("tt0133093"));
    assert_eq!(first.tmdb_id, Some(603));
    assert_eq!(first.password_protected, Some(false));
    assert_eq!(
        first.published.unwrap().to_rfc3339(),
        "2026-11-22T18:30:00+00:00"
    );
    assert_eq!(
        first.usenet_date.unwrap().to_rfc3339(),
        "2026-11-21T12:30:00+00:00"
    );

    // Age uses usenet_date (2026-11-21 12:30 UTC) when present.
    let now = Utc.with_ymd_and_hms(2026, 11, 26, 12, 30, 0).unwrap();
    assert_eq!(first.age_hours(now), Some(120));
    assert_eq!(first.age_days(now), Some(5));

    // Second item has a JPEG cover enclosure before the application/x-nzb enclosure,
    // no usenetdate (falls back to pubDate), and password="1".
    let second = &releases[1];
    assert_eq!(
        second.download_url,
        "https://api.nzbgeek.info/api?t=get&id=a1b2c3d4e5f60002&apikey=demo"
    );
    assert_eq!(second.size, Some(21_474_836_480));
    assert_eq!(second.categories, [2045]);
    assert_eq!(second.grabs, Some(7));
    assert_eq!(second.files, Some(140));
    assert_eq!(second.group.as_deref(), Some("alt.binaries.hdtv"));
    assert_eq!(second.imdb_id.as_deref(), Some("tt0133093"));
    assert_eq!(second.tmdb_id, None);
    assert_eq!(second.password_protected, Some(true));
    assert_eq!(second.usenet_date, None);
    assert_eq!(second.age_hours(now), Some(174));
    assert_eq!(second.age_days(now), Some(7));

    // Future timestamps clamp cleanly to 0.
    let earlier = Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap();
    assert_eq!(first.age_hours(earlier), Some(0));
    assert_eq!(first.age_days(earlier), Some(0));
}

#[test]
fn drunkenslug_feed_parses_offset_dates_and_link_fallback() {
    let releases = parse_feed(DRUNKENSLUG_MOVIE).unwrap();
    assert_eq!(releases.len(), 2);

    let first = &releases[0];
    assert_eq!(first.title, "Inception.2010.1080p.BluRay.DTS.x264-SLUG");
    assert_eq!(first.guid, "f9e8d7c6b5a40001");
    assert_eq!(
        first.download_url,
        "https://api.drunkenslug.example/getnzb/f9e8d7c6b5a40001.nzb&r=demo"
    );
    assert_eq!(first.size, Some(12_884_901_888));
    assert_eq!(first.categories, [2000, 2040]);
    assert_eq!(first.grabs, Some(63));
    assert_eq!(first.files, Some(115));
    assert_eq!(first.poster.as_deref(), Some("poster@slug.example"));
    assert_eq!(first.group.as_deref(), Some("alt.binaries.moovee"));
    assert_eq!(first.imdb_id.as_deref(), Some("tt1375666"));
    assert_eq!(first.tmdb_id, Some(27205));
    assert_eq!(first.password_protected, Some(false));
    assert_eq!(
        first.published.unwrap().to_rfc3339(),
        "2026-11-17T12:00:00+00:00"
    );
    // 07:00 -0500 == 12:00 UTC
    assert_eq!(
        first.usenet_date.unwrap().to_rfc3339(),
        "2026-11-16T12:00:00+00:00"
    );
    let now = Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap();
    assert_eq!(first.age_days(now), Some(10));
    assert_eq!(first.age_hours(now), Some(240));

    // Second item uses `<link>`, `<size>`, and numeric `<category>` without an `<enclosure>`.
    let second = &releases[1];
    assert_eq!(
        second.download_url,
        "https://api.drunkenslug.example/getnzb/f9e8d7c6b5a40002.nzb&r=demo"
    );
    assert_eq!(second.size, Some(34_359_738_368));
    assert_eq!(second.categories, [2050]);
    assert_eq!(second.grabs, Some(12));
    assert_eq!(second.password_protected, None);
}

#[test]
fn untyped_enclosures_boolean_flags_and_missing_dates_parse_cleanly() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
    <rss version="2.0" xmlns:newznab="http://www.newznab.com/DTD/2010/feeds/attributes/">
      <channel>
        <item>
          <title>Fallback.Release</title>
          <enclosure url="" type="application/x-nzb" />
          <enclosure url="https://indexer.example/get/1.nzb" length="512" />
          <newznab:attr name="imdb" value="0000000" />
          <newznab:attr name="password" value="yes" />
        </item>
        <item>
          <title>Clean.Release</title>
          <link>https://indexer.example/get/2.nzb</link>
          <newznab:attr name="password" value="false" />
          <newznab:attr name="usenetdate" value="2026-11-20T00:00:00Z" />
        </item>
      </channel>
    </rss>"#;
    let releases = parse_feed(xml).unwrap();
    assert_eq!(releases.len(), 2);
    assert_eq!(releases[0].guid, "Fallback.Release");
    assert_eq!(
        releases[0].download_url,
        "https://indexer.example/get/1.nzb"
    );
    assert_eq!(releases[0].size, Some(512));
    assert_eq!(releases[0].imdb_id, None);
    assert_eq!(releases[0].password_protected, Some(true));
    let now = Utc.with_ymd_and_hms(2026, 11, 26, 0, 0, 0).unwrap();
    assert_eq!(releases[0].age_hours(now), None);
    assert_eq!(releases[0].age_days(now), None);

    assert_eq!(releases[1].password_protected, Some(false));
    assert_eq!(releases[1].age_days(now), Some(6));
}

#[test]
fn error_elements_map_credentials_rate_limits_and_indexer_errors() {
    assert_eq!(parse_feed(ERROR_CREDENTIALS), Err(NewznabError::ApiKey));
    assert_eq!(parse_caps(ERROR_CREDENTIALS), Err(NewznabError::ApiKey));
    assert_eq!(
        parse_feed(r#"<error code="101" description="Account suspended" />"#),
        Err(NewznabError::ApiKey)
    );
    assert_eq!(
        parse_feed(r#"<error code="102" description="Insufficient privileges" />"#),
        Err(NewznabError::ApiKey)
    );
    assert_eq!(
        parse_feed(ERROR_RATE_LIMIT),
        Err(NewznabError::RateLimited { retry_after: None })
    );
    assert_eq!(
        parse_feed(r#"<error code="429" description="Too many requests" />"#),
        Err(NewznabError::RateLimited { retry_after: None })
    );
    assert_eq!(
        parse_feed(r#"<error code="501" description="Download limit reached" />"#),
        Err(NewznabError::RateLimited { retry_after: None })
    );
    assert_eq!(
        parse_feed(r#"<error code="203" description="Function not available" />"#),
        Err(NewznabError::Indexer {
            code: 203,
            description: "Function not available".to_owned(),
        })
    );
}

#[test]
fn invalid_xml_wrong_root_and_doctype_are_refused() {
    for body in [
        "<html><body>Login</body></html>",
        "not xml at all",
        "<rss/>x",
        r#"<?xml version="1.0"?><!DOCTYPE x [<!ENTITY a "aaaa">]><rss/>"#,
    ] {
        assert!(matches!(
            parse_feed(body),
            Err(NewznabError::Unavailable(_))
        ));
    }
    assert!(matches!(
        parse_caps(NZBGEEK_MOVIE),
        Err(NewznabError::Unavailable(_))
    ));
}

#[test]
fn error_display_messages_are_clear() {
    assert_eq!(
        NewznabError::ApiKey.to_string(),
        "the indexer refused the API key"
    );
    assert_eq!(
        NewznabError::Indexer {
            code: 202,
            description: "No such function".to_owned(),
        }
        .to_string(),
        "the indexer answered error 202: No such function"
    );
    assert_eq!(
        NewznabError::RateLimited {
            retry_after: Some(Duration::from_secs(45)),
        }
        .to_string(),
        "the indexer is rate limiting; retry in 45 s"
    );
    assert_eq!(
        NewznabError::RateLimited { retry_after: None }.to_string(),
        "the indexer is rate limiting"
    );
    assert_eq!(
        NewznabError::Url(UrlError::Address).to_string(),
        "this address is not allowed"
    );
}

fn client(server: &MockServer, token: &str) -> NewznabClient {
    NewznabClient::new(OutboundClient::new().unwrap(), &server.uri(), token)
}

fn xml(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "application/xml")
        .set_body_string(body)
}

#[tokio::test]
async fn caps_movie_search_text_search_and_rss_send_expected_params() {
    let server = MockServer::start().await;
    let fixture_key = "demo-api-token";
    Mock::given(method("GET"))
        .and(query_param("t", "caps"))
        .and(query_param("apikey", fixture_key))
        .respond_with(xml(NZBGEEK_CAPS))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .and(query_param("imdbid", "0133093"))
        .and(query_param("extended", "1"))
        .and(query_param("cat", "2000,2040"))
        .and(query_param("apikey", fixture_key))
        .respond_with(xml(NZBGEEK_MOVIE))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "search"))
        .and(query_param("q", "Inception"))
        .and(query_param("extended", "1"))
        .and(query_param("cat", "2000"))
        .and(query_param("apikey", fixture_key))
        .respond_with(xml(DRUNKENSLUG_MOVIE))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .and(query_param_is_missing("q"))
        .and(query_param_is_missing("imdbid"))
        .and(query_param("extended", "1"))
        .and(query_param("apikey", fixture_key))
        .respond_with(xml(NZBGEEK_MOVIE))
        .expect(1)
        .mount(&server)
        .await;

    let api = client(&server, fixture_key);
    let caps = api.caps().await.unwrap();
    let movies = api.search_movie(&caps, &query()).await.unwrap();
    assert_eq!(movies.len(), 2);

    let searched = api.search_text(&caps, "Inception", &[2000]).await.unwrap();
    assert_eq!(searched.len(), 2);

    let latest = api.rss(&caps, &[]).await.unwrap();
    assert_eq!(latest.len(), 2);
}

#[tokio::test]
async fn drunkenslug_movie_search_without_imdb_falls_back_to_title_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .and(query_param("q", "Inception"))
        .and(query_param("extended", "1"))
        .and(query_param_is_missing("imdbid"))
        .and(query_param_is_missing("tmdbid"))
        .respond_with(xml(DRUNKENSLUG_MOVIE))
        .expect(1)
        .mount(&server)
        .await;
    let caps = parse_caps(DRUNKENSLUG_CAPS).unwrap();
    let no_imdb = MovieQuery {
        imdb_id: None,
        tmdb_id: Some(27205),
        title: "Inception".to_owned(),
        year: Some(2010),
        categories: vec![],
    };
    let releases = client(&server, "k")
        .search_movie(&caps, &no_imdb)
        .await
        .unwrap();
    assert_eq!(releases.len(), 2);
}

#[tokio::test]
async fn wrong_key_and_rate_limit_work_across_http_and_xml_errors() {
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
            client(&server, "bad-token").caps().await.unwrap_err(),
            NewznabError::ApiKey
        );
    }

    let bare_401 = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&bare_401)
        .await;
    assert_eq!(
        client(&bare_401, "bad-token").caps().await.unwrap_err(),
        NewznabError::ApiKey
    );

    let xml_limit = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("retry-after", "90")
                .set_body_string(ERROR_RATE_LIMIT),
        )
        .mount(&xml_limit)
        .await;
    assert_eq!(
        client(&xml_limit, "k").caps().await.unwrap_err(),
        NewznabError::RateLimited {
            retry_after: Some(Duration::from_secs(90)),
        }
    );

    let http_429 = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "120"))
        .mount(&http_429)
        .await;
    assert_eq!(
        client(&http_429, "k").caps().await.unwrap_err(),
        NewznabError::RateLimited {
            retry_after: Some(Duration::from_secs(120)),
        }
    );

    let bare_429 = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&bare_429)
        .await;
    assert_eq!(
        client(&bare_429, "k").caps().await.unwrap_err(),
        NewznabError::RateLimited { retry_after: None }
    );
}

#[tokio::test]
async fn server_errors_refused_connections_and_blocked_urls_never_leak_key() {
    let fixture_token = "never-print-this-token";

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(502).set_body_string("<html>Bad gateway</html>"))
        .mount(&server)
        .await;
    let error = client(&server, fixture_token).caps().await.unwrap_err();
    let text = error.to_string();
    assert!(text.contains("502"), "{text}");
    assert!(!text.contains(fixture_token));

    let closed = MockServer::start().await;
    let api = client(&closed, fixture_token);
    drop(closed);
    let error = api.caps().await.unwrap_err();
    assert!(matches!(error, NewznabError::Unavailable(_)));
    assert!(!error.to_string().contains(fixture_token));
    assert!(!format!("{api:?}").contains(fixture_token));

    let blocked = NewznabClient::new(
        OutboundClient::new().unwrap(),
        "http://169.254.169.254/api",
        fixture_token,
    );
    assert_eq!(
        blocked.caps().await.unwrap_err(),
        NewznabError::Url(UrlError::Address)
    );
}

#[tokio::test]
async fn test_reads_caps_and_checks_key_with_one_rss_item() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("t", "caps"))
        .respond_with(xml(NZBGEEK_CAPS))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .and(query_param("limit", "1"))
        .respond_with(xml(NZBGEEK_MOVIE))
        .expect(1)
        .mount(&server)
        .await;
    let caps = client(&server, "k").test().await.unwrap();
    assert!(caps.movie_search.is_some());

    let bad_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("t", "caps"))
        .respond_with(xml(DRUNKENSLUG_CAPS))
        .mount(&bad_server)
        .await;
    Mock::given(method("GET"))
        .and(query_param("t", "movie"))
        .respond_with(xml(ERROR_CREDENTIALS))
        .mount(&bad_server)
        .await;
    assert_eq!(
        client(&bad_server, "bad").test().await.unwrap_err(),
        NewznabError::ApiKey
    );
}

#[tokio::test]
async fn caps_error_text_redacts_the_configured_key() {
    let fixture_key = "distinct-key-3f9c1d7e2b6a4c8d";
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("apikey", fixture_key))
        .respond_with(xml(&format!(
            r#"<error code="202" description="bad apikey={fixture_key}"/>"#
        )))
        .mount(&server)
        .await;

    let error = client(&server, fixture_key).caps().await.unwrap_err();
    match &error {
        NewznabError::Indexer { code, description } => {
            assert_eq!(*code, 202);
            assert!(description.contains("[redacted]"), "{description}");
            assert!(!description.contains(fixture_key));
        }
        other => panic!("expected an indexer error, got {other:?}"),
    }
    assert!(!format!("{error:?}").contains(fixture_key));
    assert!(!error.to_string().contains(fixture_key));
}

#[tokio::test]
async fn http_error_body_redacts_the_configured_key() {
    let fixture_key = "distinct-key-3f9c1d7e2b6a4c8d";
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(500)
                .insert_header("content-type", "text/plain")
                .set_body_string(format!("upstream rejected apikey {fixture_key}")),
        )
        .mount(&server)
        .await;

    let error = client(&server, fixture_key).caps().await.unwrap_err();
    match &error {
        NewznabError::Unavailable(reason) => {
            assert!(reason.contains("[redacted]"), "{reason}");
            assert!(!reason.contains(fixture_key));
        }
        other => panic!("expected an unavailable error, got {other:?}"),
    }
    assert!(!format!("{error:?}").contains(fixture_key));
    assert!(!error.to_string().contains(fixture_key));
}
