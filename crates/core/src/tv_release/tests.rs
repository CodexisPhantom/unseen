use super::*;

/// Rows of the table under `### TV test table` in docs/07, as trimmed cells.
fn doc_tv_rows() -> Vec<Vec<String>> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/07-release-scoring.md"
    );
    let doc = std::fs::read_to_string(path).expect("docs/07 is readable");
    doc.lines()
        .skip_while(|line| line.trim() != "### TV test table")
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .filter(|line| line.starts_with('|'))
        .skip(2) // header and separator
        .map(|line| {
            line.trim()
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

/// Checks the Numbering cell against the parsed seasons, episodes,
/// absolute_episodes, and air_date.
fn numbering_holds(spec: &str, parsed: &ParsedTvRelease) -> bool {
    // Combined standard + absolute (`S01E12, absolute 12`).
    if let Some((se_part, abs_part)) = spec.split_once(", absolute ") {
        let abs: u16 = abs_part.parse().expect("valid abs number");
        return numbering_holds(se_part, parsed) && parsed.absolute_episodes == [abs];
    }

    // Daily date (`YYYY-MM-DD`).
    if spec.len() == 10 && spec.as_bytes()[4] == b'-' && spec.as_bytes()[7] == b'-' {
        return parsed.air_date.as_deref() == Some(spec)
            && parsed.seasons.is_empty()
            && parsed.episodes.is_empty()
            && parsed.absolute_episodes.is_empty();
    }

    // Absolute single or range (`absolute 12`, `absolute 5 to 8`).
    if let Some(rest) = spec.strip_prefix("absolute ") {
        let expected: Vec<u16> = if let Some((a, b)) = rest.split_once(" to ") {
            let start: u16 = a.parse().expect("abs start");
            let end: u16 = b.parse().expect("abs end");
            (start..=end).collect()
        } else {
            vec![rest.parse().expect("abs single")]
        };
        return parsed.absolute_episodes == expected
            && parsed.episodes.is_empty()
            && parsed.air_date.is_none();
    }

    // Multi-season range (`S01 to S03`).
    if let Some((a, b)) = spec.split_once(" to S") {
        let start: u16 = a.strip_prefix('S').expect("S").parse().expect("season");
        let end: u16 = b.parse().expect("season");
        let expected: Vec<u16> = (start..=end).collect();
        return parsed.seasons == expected && parsed.episodes.is_empty();
    }

    // Multi-episode range (`S03E04 to E06`).
    if let Some((se, end_e)) = spec.split_once(" to E") {
        let (s, e1) = se.strip_prefix('S').expect("S").split_once('E').expect("E");
        let season: u16 = s.parse().expect("season");
        let start: u16 = e1.parse().expect("ep start");
        let end: u16 = end_e.parse().expect("ep end");
        let expected: Vec<u16> = (start..=end).collect();
        return parsed.seasons == [season] && parsed.episodes == expected;
    }

    // Multi-episode list (`S03E04, E05`).
    if let Some((se, second_e)) = spec.split_once(", E") {
        let (s, e1) = se.strip_prefix('S').expect("S").split_once('E').expect("E");
        let season: u16 = s.parse().expect("season");
        let ep1: u16 = e1.parse().expect("ep1");
        let ep2: u16 = second_e.parse().expect("ep2");
        return parsed.seasons == [season] && parsed.episodes == [ep1, ep2];
    }

    // Standard single episode (`S03E04`).
    if let Some(rest) = spec.strip_prefix('S') {
        if let Some((s, e)) = rest.split_once('E') {
            let season: u16 = s.parse().expect("season");
            let ep: u16 = e.parse().expect("ep");
            return parsed.seasons == [season] && parsed.episodes == [ep];
        }
        // Single season (`S03`).
        let season: u16 = rest.parse().expect("season");
        return parsed.seasons == [season] && parsed.episodes.is_empty();
    }

    false
}

/// Checks one comma-separated item of the Notes column.
fn note_holds(note: &str, parsed: &ParsedTvRelease) -> bool {
    if let Some(year) = note.strip_prefix("year ") {
        let y: u16 = year.parse().expect("year");
        return parsed.year == Some(y);
    }
    if let Some(ver) = note.strip_prefix("version ") {
        let v: u8 = ver.parse().expect("version");
        return parsed.version == v;
    }
    if let Some(hash) = note.strip_prefix("hash ") {
        return parsed.hash.as_deref() == Some(hash);
    }
    if let Some(lang) = note.strip_prefix("language ") {
        return parsed
            .languages
            .iter()
            .any(|l| format!("{l:?}").eq_ignore_ascii_case(lang));
    }
    if let Some(part) = note.strip_prefix("partial season part ") {
        let p: u16 = part.parse().expect("part");
        return parsed.is_partial_season
            && parsed.part == Some(p)
            && parsed.release_type == TvReleaseType::Unknown;
    }
    if let Some(part) = note.strip_prefix("part ") {
        let p: u16 = part.parse().expect("part");
        return parsed.part == Some(p);
    }
    if let Some(rest) = note.strip_prefix("Sonarr calls it `") {
        let expected = rest.trim_end_matches('`');
        return parsed.quality.sonarr_name() == Some(expected);
    }
    match note {
        "multi-episode" => parsed.release_type == TvReleaseType::MultiEpisode,
        "season pack" | "batch" => parsed.release_type == TvReleaseType::SeasonPack,
        "multi-season" => parsed.release_type == TvReleaseType::MultiSeasonPack,
        "split episode" => parsed.is_split_episode,
        "special" => parsed.is_special,
        "daily" => parsed.air_date.is_some() && parsed.release_type == TvReleaseType::Episode,
        "proper" => parsed.proper && parsed.version >= 2,
        "repack" => parsed.repack && parsed.version >= 2,
        "HDR10" => parsed.hdr.contains(&Hdr::Hdr10),
        "HDR10+" => parsed.hdr.contains(&Hdr::Hdr10Plus),
        "DV" => parsed.hdr.contains(&Hdr::DolbyVision),
        "Atmos" => parsed.audio.contains(&Audio::Atmos),
        "x264" => parsed.codec == Some(Codec::X264),
        "x265" => parsed.codec == Some(Codec::X265),
        "AV1" => parsed.codec == Some(Codec::Av1),
        "hardcoded subs" => parsed.hardcoded_subs,
        _ => true,
    }
}

#[test]
fn every_row_of_the_doc_07_tv_test_table_passes() {
    let rows = doc_tv_rows();
    assert!(
        rows.len() >= 80,
        "TV table must have at least 80 rows, found {}",
        rows.len()
    );
    let mut failures = Vec::new();
    for row in &rows {
        let [name, title, numbering, expected_quality, group, notes] = row.as_slice() else {
            failures.push(format!("not 6 cells: {row:?}"));
            continue;
        };
        let parsed = parse(name.trim_matches('`'));
        let expected_group = if group.is_empty() {
            None
        } else {
            Some(group.as_str())
        };
        let num_ok = numbering_holds(numbering, &parsed);
        let notes_ok = notes
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .all(|n| note_holds(n, &parsed));
        if parsed.title != *title
            || !num_ok
            || parsed.quality.name() != expected_quality
            || !parsed.quality.is_tv()
            || parsed.release_group.as_deref() != expected_group
            || !notes_ok
        {
            failures.push(format!(
                "{row:?}\n  got title={:?} year={:?} seasons={:?} eps={:?} abs={:?} air={:?} type={:?} split={} special={} partial={} part={:?} quality={} group={:?} hash={:?} ver={} proper={} repack={} lang={:?} codec={:?} audio={:?} hdr={:?}",
                parsed.title,
                parsed.year,
                parsed.seasons,
                parsed.episodes,
                parsed.absolute_episodes,
                parsed.air_date,
                parsed.release_type,
                parsed.is_split_episode,
                parsed.is_special,
                parsed.is_partial_season,
                parsed.part,
                parsed.quality.name(),
                parsed.release_group,
                parsed.hash,
                parsed.version,
                parsed.proper,
                parsed.repack,
                parsed.languages,
                parsed.codec,
                parsed.audio,
                parsed.hdr,
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn tv_quality_variants_and_fallbacks() {
    let cases = [
        // Sources & modifiers
        ("Show.S01E01.1080p.WEB-DL.x264-GRP", Quality::WebDl1080p),
        ("Show.S01E01.720p.WEB-DL.x264-GRP", Quality::WebDl720p),
        ("Show.S01E01.2160p.WEB-DL.x265-GRP", Quality::WebDl2160p),
        ("Show.S01E01.480p.WEB-DL.x264-GRP", Quality::WebDl480p),
        ("Show.S01E01.[WEBDL].mkv", Quality::WebDl720p),
        ("Show.S01E01.1080p.MKV.WEB", Quality::WebDl1080p),
        ("Show.S01E01.720p.MKV.WEB", Quality::WebDl720p),
        ("Show.S01E01.2160p.MKV.WEB", Quality::WebDl2160p),
        ("Show.S01E01.MKV.WEB", Quality::WebDl480p),
        ("Show.S01E01.480p.WEBRip.x264-GRP", Quality::WebRip480p),
        ("Show.S01E01.720p.WEBRip.x264-GRP", Quality::WebRip720p),
        ("Show.S01E01.1080p.WEBRip.x264-GRP", Quality::WebRip1080p),
        ("Show.S01E01.2160p.WEBRip.x265-GRP", Quality::WebRip2160p),
        ("Show.S01E01.BluRay.XviD-GRP", Quality::Bluray480p),
        ("Show.S01E01.BluRay.x264-GRP", Quality::Bluray720p),
        ("Show.S01E01.BluRay.REMUX.AVC-GRP", Quality::Remux1080p),
        ("Show.S01E01.720p.BluRay.REMUX.AVC-GRP", Quality::Bluray720p),
        ("Show.S01E01.480p.REMUX.AVC-GRP", Quality::Bluray480p),
        ("Show.S01E01.720p.REMUX.AVC-GRP", Quality::Bluray720p),
        ("Show.S01E01.1080p.REMUX.AVC-GRP", Quality::Remux1080p),
        ("Show.S01E01.2160p.REMUX.HEVC-GRP", Quality::Remux2160p),
        ("Show.S01E01.BDRip.x264-GRP", Quality::Bluray480p),
        ("Show.S01E01.720p.BDRip.x264-GRP", Quality::Bluray720p),
        ("Show.S01E01.1080p.BDRip.x264-GRP", Quality::Bluray1080p),
        ("Show.S01E01.2160p.BDRip.x265-GRP", Quality::Bluray2160p),
        ("Show.S01E01.[BD720p].mkv", Quality::Bluray720p),
        ("Show.S01E01.[BD1080p].mkv", Quality::Bluray1080p),
        ("Show.S01E01.[BD2160p].mkv", Quality::Bluray2160p),
        ("Show.S01E01.[Bluray1080p].REMUX.mkv", Quality::Remux1080p),
        ("Show.S01E01.[Bluray2160p].REMUX.mkv", Quality::Remux2160p),
        ("Show.S01E01.[HDTV].mkv", Quality::Hdtv720p),
        ("Show.S01E01.WS.PDTV.x264-GRP", Quality::Sdtv),
        ("Show.S01E01.HR.WS.PDTV.x264-GRP", Quality::Hdtv720p),
        ("Show.S01E01.720p.TVRip.x264-GRP", Quality::Hdtv720p),
        ("Show.S01E01.1080p.TVRip.x264-GRP", Quality::Hdtv1080p),
        ("Show.S01E01.HD.TV.mkv", Quality::Hdtv720p),
        ("Show.S01E01.SD.TV.avi", Quality::Sdtv),
        ("Show.S01E01.360p.mkv", Quality::Sdtv),
        ("Show.S01E01.540p.mkv", Quality::Sdtv),
        ("Show.S01E01.576p.mkv", Quality::Sdtv),
        ("Show.S01E01.2160p.mkv", Quality::Hdtv2160p),
        ("Show.S01E01.UHD.mkv", Quality::Hdtv2160p),
        // `.m2ts` extension with and without resolution
        ("Show.S01E01.m2ts", Quality::Bluray720p),
        ("Show.S01E01.480p.m2ts", Quality::Bluray480p),
        ("Show.S01E01.720p.m2ts", Quality::Bluray720p),
        ("Show.S01E01.1080p.m2ts", Quality::Bluray1080p),
        ("Show.S01E01.2160p.m2ts", Quality::Bluray2160p),
        // Other extension fallbacks
        ("Show.S01E01.mkv", Quality::Hdtv720p),
        ("Show.S01E01.avi", Quality::Sdtv),
        ("Show.S01E01.iso", Quality::Dvd),
        ("Show.S01E01.webm", Quality::Unknown),
        ("Show.S01E01.The.Web.MT-dd", Quality::Unknown),
        // Movie-only qualities must never be returned
        ("Show.S01E01.WORKPRINT.XviD-GRP", Quality::Unknown),
        ("Show.S01E01.HDCAM.XviD-GRP", Quality::Unknown),
        ("Show.S01E01.TELESYNC.XviD-GRP", Quality::Unknown),
        ("Show.S01E01.TELECINE.XviD-GRP", Quality::Unknown),
        ("Show.S01E01.R5.XviD-GRP", Quality::Unknown),
        ("Show.S01E01.DVDSCR.XviD-GRP", Quality::Unknown),
        ("Show.S01E01.DVD-R.XviD-GRP", Quality::Dvd),
        ("Show.S01E01.COMPLETE.BLURAY-GRP", Quality::Bluray720p),
    ];
    for (name, expected) in cases {
        let parsed = parse(name);
        assert!(parsed.quality.is_tv(), "{name} produced non-TV quality");
        assert_eq!(parsed.quality, expected, "{name}");
    }
}

#[test]
fn anime_specials_recaps_combined_and_mini_series_forms() {
    // Combined absolute + standard (both directions)
    let p = parse("[K-F] Naruto S10E14 214 [720p]");
    assert_eq!(p.title, "Naruto");
    assert_eq!(p.seasons, [10]);
    assert_eq!(p.episodes, [14]);
    assert_eq!(p.absolute_episodes, [214]);
    assert_eq!(p.release_group.as_deref(), Some("K-F"));

    let p = parse("[RlsGrp] Show - S01E01-02 - 001-002 [720p]");
    assert_eq!(p.seasons, [1]);
    assert_eq!(p.episodes, [1, 2]);
    assert_eq!(p.absolute_episodes, [1, 2]);
    assert_eq!(p.release_type, TvReleaseType::MultiEpisode);

    // Decimal recap special + S00E01
    let p = parse("[sam] Anime - 15.5 (S00E01) [BD 1080p FLAC] [3E8D676D]");
    assert_eq!(p.title, "Anime");
    assert_eq!(p.seasons, [0]);
    assert_eq!(p.episodes, [1]);
    assert!(p.absolute_episodes.is_empty());
    assert!(p.is_special);
    assert_eq!(p.hash.as_deref(), Some("3E8D676D"));

    // Anime decimal recap without SxxExx
    let p = parse("[SubsPlease] Frieren - 12.5 (1080p) [29AF1C23].mkv");
    assert_eq!(p.title, "Frieren");
    assert!(p.absolute_episodes.is_empty());
    assert!(p.is_special);

    // Anime unnumbered OVA / NCOP
    let p = parse("[Underwater] Another - OVA (BD 1080p) [3A561D0E].mkv");
    assert_eq!(p.title, "Another");
    assert!(p.is_special);
    assert_eq!(p.release_type, TvReleaseType::Unknown);
    assert_eq!(p.quality, Quality::Bluray1080p);

    // Anime (Season 2) - 33
    let p = parse("[DameDesuYo] Show (Season 2) - 33 (1280x720) [42A12A76].mkv");
    assert_eq!(p.title, "Show");
    assert_eq!(p.seasons, [2]);
    assert_eq!(p.absolute_episodes, [33]);

    // Anime bracketed episode number
    let p = parse("[XKsub] Show S2 [05][1080p]");
    assert_eq!(p.title, "Show S2");
    assert_eq!(p.absolute_episodes, [5]);

    let p = parse("Series_Title_2_[01-05]_[AniLibria_TV]_[WEBRip_1080p]");
    assert_eq!(p.title, "Series Title 2");
    assert_eq!(p.absolute_episodes, [1, 2, 3, 4, 5]);
    assert_eq!(p.release_type, TvReleaseType::MultiEpisode);

    // Anime without subgroup: Episode keyword, Ep range, trailing group in brackets
    let p = parse("Show - 031 - Episode Title [Lunar].avi");
    assert_eq!(p.title, "Show");
    assert_eq!(p.absolute_episodes, [31]);
    assert_eq!(p.release_group.as_deref(), Some("Lunar"));

    let p = parse("Show.Ep01-12.Complete.1080p.BluRay.x264-GROUP");
    assert_eq!(p.title, "Show");
    assert_eq!(p.absolute_episodes, (1..=12).collect::<Vec<_>>());
    assert_eq!(p.release_type, TvReleaseType::SeasonPack);

    let p = parse("Show Episode 56 [VOSTFR V2][720p][AAC]-Mystic");
    assert_eq!(p.title, "Show");
    assert_eq!(p.absolute_episodes, [56]);
    assert_eq!(p.version, 2);
    assert_eq!(p.languages, [Language::French]);

    // Mini-series forms
    let p = parse("Show.Title.E07-E08.1080p.WEB-DL-GROUP");
    assert_eq!(p.seasons, [1]);
    assert_eq!(p.episodes, [7, 8]);
    assert_eq!(p.release_type, TvReleaseType::MultiEpisode);

    let p = parse("Show.Made.in.Canada.Part.Two.720p.HDTV.x264-2HD");
    assert_eq!(p.title, "Show Made in Canada");
    assert_eq!(p.seasons, [1]);
    assert_eq!(p.episodes, [2]);

    let p = parse("Show.Title.5of9.DVDRip.x264-GRP");
    assert_eq!(p.title, "Show Title");
    assert_eq!(p.seasons, [1]);
    assert_eq!(p.episodes, [5]);

    let p = parse("Show.2016.E04.Power.720p.WEB-DL.x264-MARS");
    assert_eq!(p.title, "Show");
    assert_eq!(p.year, Some(2016));
    assert_eq!(p.seasons, [1]);
    assert_eq!(p.episodes, [4]);

    // Glued `SxxExx` without separator
    let p = parse("grp-zoos01e11e12-1080p");
    assert_eq!(p.seasons, [1]);
    assert_eq!(p.episodes, [11, 12]);
    assert_eq!(p.release_type, TvReleaseType::MultiEpisode);
}

#[test]
fn special_edge_cases_revisions_dates_and_groups() {
    // Full-season via `S01E01-08 of 8`
    let p = parse("Show (S01E01-08 of 8) 1080p WEB-DL");
    assert_eq!(p.seasons, [1]);
    assert!(p.episodes.is_empty());
    assert_eq!(p.release_type, TvReleaseType::SeasonPack);

    // Season with Special tag is not a full season pack
    let p = parse("Show.S02.Special.720p.HDTV.x264-GRP");
    assert_eq!(p.seasons, [2]);
    assert!(p.is_special);
    assert_eq!(p.release_type, TvReleaseType::Unknown);

    // Bare season folder is rejected into fallback Unknown
    let p = parse("Season 3");
    assert_eq!(p.release_type, TvReleaseType::Unknown);
    assert!(p.seasons.is_empty());

    // Ambiguous DD.MM.YYYY is not parsed as daily
    let p = parse("Show.Title.12.02.2025.1080p.HDTV.x264-GRP");
    assert_eq!(p.air_date, None);

    // Invalid calendar date (Feb 30) is rejected
    let p = parse("Show.Title.2024.02.30.1080p.HDTV.x264-GRP");
    assert_eq!(p.air_date, None);

    // Pre-1970 daily date is rejected
    let p = parse("Show.Title.1955.04.12.720p.HDTV.x264-GRP");
    assert_eq!(p.air_date, None);

    // Airdate immediately before SxxExx strips airdate from title
    let p = parse("Judge Developer 2016 02 25 S20E142 720p HDTV");
    assert_eq!(p.title, "Judge Developer");
    assert_eq!(p.seasons, [20]);
    assert_eq!(p.episodes, [142]);

    // Trailing `[Dolby Vision]` leading bracket preserved as HDR, not subgroup
    let p = parse("[Dolby Vision] Show.S07.MULTi.UHD.BLURAY.REMUX.DV-NoTag");
    assert_eq!(p.title, "Show");
    assert_eq!(p.seasons, [7]);
    assert_eq!(p.release_group.as_deref(), Some("NoTag"));
    assert!(p.hdr.contains(&Hdr::DolbyVision));
    assert_eq!(p.quality, Quality::Remux2160p);

    // Request prefix `[site][12345]` stripped from title
    let p = parse("[scnzbefnet][509103] 2.Developers.S03E18.720p.HDTV.x264-DIMENSION");
    assert_eq!(p.title, "2 Developers");
    assert_eq!(p.release_group.as_deref(), Some("DIMENSION"));

    // Trailing language word after group (`-SKGTV English`)
    let p = parse("Show.S02E04.720p.WEBRip.x264-SKGTV English");
    assert_eq!(p.release_group.as_deref(), Some("SKGTV"));

    // Pure numeric group `-666` is rejected; `MULTILANG` maps to Multi
    let p = parse("Show.S01E01.MULTILANG.1080p.WEB-DL.x264-666");
    assert_eq!(p.release_group, None);
    assert_eq!(p.languages, [Language::Multi]);

    // Language word inside show title is NOT parsed as release language
    let p = parse("The.French.Connection.S01E01.1080p.WEB-DL.x264-GROUP");
    assert_eq!(p.title, "The French Connection");
    assert!(p.languages.is_empty());

    // `v2` + `PROPER` increments version; `[v3]` works; `PROPER2` -> 3
    let p = parse("[Group] Show - 05v2 PROPER (1080p)");
    assert!(p.proper);
    assert_eq!(p.version, 3);

    let p = parse("Show.S01E01.PROPER2.1080p.WEB-DL.x264-GRP");
    assert!(p.proper);
    assert_eq!(p.version, 3);

    let p = parse("Show.S01E01.[v3].1080p.WEB-DL.x264-GRP");
    assert_eq!(p.version, 3);

    // Episode title hyphenated number (`-31st`) does not corrupt episode range
    let p = parse("Show-S07E12-31st_Century_Fox-[Bluray-1080p].mkv");
    assert_eq!(p.seasons, [7]);
    assert_eq!(p.episodes, [12]);
    assert_eq!(p.quality, Quality::Bluray1080p);

    // Full-width anime brackets normalized
    let p = parse("【SubsPlease】 Frieren - 04 (1080p) 【ABCD1234】.mkv");
    assert_eq!(p.title, "Frieren");
    assert_eq!(p.absolute_episodes, [4]);
    assert_eq!(p.release_group.as_deref(), Some("SubsPlease"));
    assert_eq!(p.hash.as_deref(), Some("ABCD1234"));

    // All 12 short months & all word numbers
    for (mon, expected) in [
        ("Jan", "2024-01-01"),
        ("Feb", "2024-02-02"),
        ("Mar", "2024-03-03"),
        ("Apr", "2024-04-04"),
        ("May", "2024-05-05"),
        ("Jun", "2024-06-06"),
        ("Jul", "2024-07-07"),
        ("Aug", "2024-08-08"),
        ("Sep", "2024-09-09"),
        ("Oct", "2024-10-10"),
        ("Nov", "2024-11-11"),
        ("Dec", "2024-12-12"),
    ] {
        let name = format!("Show.1st.{mon}.2024.720p.HDTV-GRP");
        let _ = parse(&name);
        let day = &expected[8..10];
        let ordinal = match day {
            "01" => "1st",
            "02" => "2nd",
            "03" => "3rd",
            other => other.trim_start_matches('0'),
        };
        let suffix = if ordinal.ends_with(['t', 'd']) {
            ""
        } else {
            "th"
        };
        let exact = format!("Show.{ordinal}{suffix}.{mon}.2024.720p.HDTV-GRP");
        assert_eq!(parse(&exact).air_date.as_deref(), Some(expected), "{exact}");
    }

    for (word, num) in [
        ("One", 1),
        ("Two", 2),
        ("Three", 3),
        ("Four", 4),
        ("Five", 5),
        ("Six", 6),
        ("Seven", 7),
        ("Eight", 8),
        ("Nine", 9),
    ] {
        let name = format!("Miniseries.Part.{word}.720p.HDTV-GRP");
        assert_eq!(parse(&name).episodes, [num], "{name}");
    }
}

#[test]
fn odd_and_adversarial_inputs_never_panic() {
    let long = "Show.".repeat(10_000) + "S01E01.1080p.WEB-DL.x264-GROUP";
    let odd = [
        "",
        " ",
        "...",
        "-_. -_.",
        "-",
        "[]",
        "()",
        "[[[",
        "S01E01-65000",
        "S01-S999",
        "[Group] Show (01-65000)",
        "S01E05-02",
        "2024.99.99",
        "1080p",
        "-GROUP",
        "【】",
        "Босх.S02E01.1080p.WEB-DL.x264-ГРУППА",
        "葬送のフリーレン - 12 (1080p) [ABCD1234].mkv",
        "Amélie_S01E01_1080p\u{0301}🎬-é",
        "\0\n\t\r",
        long.as_str(),
    ];
    for name in odd {
        let parsed = parse(name);
        assert!(parsed.quality.is_tv(), "{name:?}");
        assert!(parsed.version >= 1, "{name:?}");
    }
    assert_eq!(parse("").title, "");
    assert_eq!(parse(&long).seasons, [1]);
    assert_eq!(parse(&long).episodes, [1]);
}

/// Deterministic fuzz test: every random combination of TV tokens must never
/// panic and must always produce a valid TV quality.
#[test]
fn random_tv_names_never_panic_and_always_yield_tv_quality() {
    const PARTS: &[&str] = &[
        "The",
        "Expanse",
        "2005",
        "2024",
        "24",
        "S01E01",
        "S03E04-E06",
        "S00E01",
        "S01E01A",
        "S01",
        "S01-S03",
        "Season",
        "3",
        "Part",
        "1",
        "3x04",
        "2024.05.17",
        "12v2",
        "(01-28)",
        "[SubsPlease]",
        "[ABCD1234]",
        "[Batch]",
        "1080p",
        "2160p",
        "720p",
        "480p",
        "BluRay",
        "WEB-DL",
        "WEBRip",
        "WEB",
        "REMUX",
        "HDTV",
        "CAM",
        "BR-DISK",
        "DVD-R",
        "DVDSCR",
        "x264",
        "H.265",
        "HEVC",
        "DDP5.1",
        "Atmos",
        "HDR10+",
        "DV",
        "PROPER",
        "REPACK2",
        "FRENCH",
        "MULTi",
        "-GROUP",
        ".mkv",
        ".mp4",
        "(",
        ")",
        "[",
        "]",
        "-",
        ".",
        " ",
        "_",
        "é",
        "千",
        "🎬",
        "",
    ];
    let mut state: u64 = 90;
    let mut next = |bound: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as usize % bound
    };
    for _ in 0..5_000 {
        let name: String = (0..next(12)).map(|_| PARTS[next(PARTS.len())]).collect();
        let parsed = parse(&name);
        assert!(parsed.quality.is_tv(), "{name:?}");
        assert!(parsed.version >= 1, "{name:?}");
    }
}
