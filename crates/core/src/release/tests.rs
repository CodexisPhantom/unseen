use super::*;

fn quality(parsed: &ParsedRelease) -> &'static str {
    quality_name(parsed.source, parsed.modifier, parsed.resolution)
}

/// Rows of the table under `## Test table` in docs/07, as trimmed cells.
fn doc_rows() -> Vec<Vec<String>> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/07-release-scoring.md"
    );
    let doc = std::fs::read_to_string(path).expect("docs/07 is readable");
    doc.lines()
        .skip_while(|line| line.trim() != "## Test table")
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

/// Checks one comma-separated item of the Notes column. Items that are not
/// an assertion (`bracket style`, `year-like title`) are comments.
fn note_holds(note: &str, parsed: &ParsedRelease) -> bool {
    if let Some(edition) = note.strip_prefix("edition ") {
        return parsed.edition.as_deref() == Some(edition);
    }
    if let Some(language) = note.strip_prefix("language ") {
        return parsed
            .languages
            .iter()
            .any(|l| format!("{l:?}") == language);
    }
    match note {
        "HDR10" => parsed.hdr.contains(&Hdr::Hdr10),
        "HDR10+" => parsed.hdr.contains(&Hdr::Hdr10Plus),
        "DV" => parsed.hdr.contains(&Hdr::DolbyVision),
        "Atmos" => parsed.audio.contains(&Audio::Atmos),
        "proper" => parsed.proper && parsed.version == 2,
        "repack" => parsed.repack && parsed.version == 2,
        _ => true,
    }
}

#[test]
fn every_row_of_the_doc_07_test_table_passes() {
    let rows = doc_rows();
    assert!(
        rows.len() >= 16,
        "table not found or shrunk: {} rows",
        rows.len()
    );
    let mut failures = Vec::new();
    for row in &rows {
        let [name, title, year, expected_quality, group, notes] = row.as_slice() else {
            failures.push(format!("not 6 cells: {row:?}"));
            continue;
        };
        let parsed = parse(name.trim_matches('`'));
        let notes_hold = notes
            .split(',')
            .all(|note| note_holds(note.trim(), &parsed));
        if parsed.title != *title
            || parsed.year != year.parse().ok()
            || quality(&parsed) != expected_quality
            || parsed.release_group.as_deref() != Some(group.as_str())
            || !notes_hold
        {
            failures.push(format!(
                "{row:?}\n  got {parsed:?} quality {}",
                quality(&parsed)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn title_year_quality_and_group() {
    // (name, title, year, quality, group)
    type Case = (
        &'static str,
        &'static str,
        Option<u16>,
        &'static str,
        Option<&'static str>,
    );
    let cases: &[Case] = &[
        (
            "Blade.Runner.2049.2017.2160p.UHD.BluRay.x265-TERMiNAL",
            "Blade Runner 2049",
            Some(2017),
            "Bluray-2160p",
            Some("TERMiNAL"),
        ),
        (
            "1917.2019.1080p.BluRay.x264-SPARKS",
            "1917",
            Some(2019),
            "Bluray-1080p",
            Some("SPARKS"),
        ),
        (
            "2012.1080p.BluRay.x264-GROUP",
            "2012",
            None,
            "Bluray-1080p",
            Some("GROUP"),
        ),
        (
            "Some.Movie.720p.WEB-DL.x264-GROUP",
            "Some Movie",
            None,
            "WEBDL-720p",
            Some("GROUP"),
        ),
        (
            "Spider-Man.2002.1080p.BluRay.x264-GROUP",
            "Spider-Man",
            Some(2002),
            "Bluray-1080p",
            Some("GROUP"),
        ),
        ("Spider-Man", "Spider-Man", None, "Unknown", None),
        (
            "Charlottes.Web.2006.720p.HDTV.x264-GROUP",
            "Charlottes Web",
            Some(2006),
            "HDTV-720p",
            Some("GROUP"),
        ),
        (
            "Some_Movie_2019_1080p_WEBRip_x264-GROUP.mkv",
            "Some Movie",
            Some(2019),
            "WEBRip-1080p",
            Some("GROUP"),
        ),
        (
            "[www.example.com] - Some.Movie.2019.1080p.BluRay.x264-GROUP",
            "Some Movie",
            Some(2019),
            "Bluray-1080p",
            Some("GROUP"),
        ),
        (
            "Some.Movie.2019.1080p.WEB-DL",
            "Some Movie",
            Some(2019),
            "WEBDL-1080p",
            None,
        ),
        (
            "Some.Movie.2019.1080p.BluRay.DTS-HD.MA.5.1",
            "Some Movie",
            Some(2019),
            "Bluray-1080p",
            None,
        ),
        (
            "Some Movie (2019) [BluRay] [1080p]",
            "Some Movie",
            Some(2019),
            "Bluray-1080p",
            None,
        ),
        (
            "Some.Movie.2019.1080p.BluRay.x264-GROUP[rarbg]",
            "Some Movie",
            Some(2019),
            "Bluray-1080p",
            Some("GROUP"),
        ),
        ("Some.Movie.2019", "Some Movie", Some(2019), "Unknown", None),
        (
            "(500).Days.of.Summer.2009.BDRip.x264-GROUP",
            "(500) Days of Summer",
            Some(2009),
            "Bluray-720p",
            Some("GROUP"),
        ),
    ];
    for (name, title, year, expected_quality, group) in cases {
        let parsed = parse(name);
        assert_eq!(parsed.title, *title, "{name}");
        assert_eq!(parsed.year, *year, "{name}");
        assert_eq!(quality(&parsed), *expected_quality, "{name}");
        assert_eq!(parsed.release_group.as_deref(), *group, "{name}");
    }
}

#[test]
fn quality_of_each_source_tag() {
    let cases = [
        ("WORKPRINT", "WORKPRINT"),
        ("CAM", "CAM"),
        ("HDCAM", "CAM"),
        ("CAMRip", "CAM"),
        ("TELESYNC", "TELESYNC"),
        ("HDTS", "TELESYNC"),
        ("TC", "TELECINE"),
        ("TELECINE", "TELECINE"),
        ("R5", "REGIONAL"),
        ("DVDSCR", "DVDSCR"),
        ("SCREENER", "DVDSCR"),
        ("SDTV", "SDTV"),
        ("HDTV", "SDTV"),
        ("DVDRip", "DVD"),
        ("DVD", "DVD"),
        ("DVD-R", "DVD-R"),
        ("DVD9", "DVD-R"),
        ("DVD.ISO", "DVD"),
        ("WEB-DL", "WEBDL-480p"),
        ("WEBRip", "WEBRip-480p"),
        ("480p.BluRay", "Bluray-480p"),
        ("576p.BluRay", "Bluray-576p"),
        ("BluRay", "Bluray-720p"),
        ("720p.WEBRip", "WEBRip-720p"),
        ("720p.Blu-Ray", "Bluray-720p"),
        ("1080p.HDTV", "HDTV-1080p"),
        ("1080i.HDTV", "HDTV-1080p"),
        ("1080p.WEB.DL", "WEBDL-1080p"),
        ("1080p.BRRip", "Bluray-1080p"),
        ("1080p.BDRemux", "Remux-1080p"),
        ("1080p.REMUX", "Remux-1080p"),
        ("BluRay.REMUX", "Remux-1080p"),
        ("720p.BluRay.REMUX", "Bluray-720p"),
        ("2160p.HDTV", "HDTV-2160p"),
        ("2160p.WEBRip", "WEBRip-2160p"),
        ("4K.WEB", "WEBDL-2160p"),
        ("UHD.BluRay", "Bluray-2160p"),
        ("2160p.UHD.BluRay.REMUX", "Remux-2160p"),
        ("BD50", "BR-DISK"),
        ("1080p.BluRay.ISO", "BR-DISK"),
        ("1080p.RawHD", "Raw-HD"),
        ("1080p", "Unknown"),
        ("360p", "Unknown"),
        ("", "Unknown"),
    ];
    for (tags, expected) in cases {
        let name = format!("Some.Movie.2020.{tags}.x264-GROUP");
        assert_eq!(quality(&parse(&name)), expected, "{name}");
    }
}

#[test]
fn codec_audio_and_hdr_tags() {
    type Case = (
        &'static str,
        Option<Codec>,
        &'static [Audio],
        &'static [Hdr],
    );
    let cases: &[Case] = &[
        ("x264.AAC2.0", Some(Codec::X264), &[Audio::Aac], &[]),
        ("H.264.DD5.1", Some(Codec::X264), &[Audio::Ac3], &[]),
        ("AVC.AC3", Some(Codec::X264), &[Audio::Ac3], &[]),
        ("x265.DD+5.1", Some(Codec::X265), &[Audio::Eac3], &[]),
        (
            "HEVC.EAC3.HDR10+",
            Some(Codec::X265),
            &[Audio::Eac3],
            &[Hdr::Hdr10Plus],
        ),
        (
            "H265.DDP5.1.Atmos.HDR10Plus.DoVi",
            Some(Codec::X265),
            &[Audio::Eac3, Audio::Atmos],
            &[Hdr::Hdr10Plus, Hdr::DolbyVision],
        ),
        (
            "AV1.DTS.HDR10",
            Some(Codec::Av1),
            &[Audio::Dts],
            &[Hdr::Hdr10],
        ),
        (
            "XviD.DTS-HD.MA.7.1",
            Some(Codec::Xvid),
            &[Audio::DtsHdMa],
            &[],
        ),
        (
            "TrueHD.7.1.Atmos.Dolby.Vision",
            None,
            &[Audio::TrueHd, Audio::Atmos],
            &[Hdr::DolbyVision],
        ),
        ("1080p", None, &[], &[]),
    ];
    for (tags, codec, audio, hdr) in cases {
        let name = format!("Some.Movie.2020.{tags}-GROUP");
        let parsed = parse(&name);
        assert_eq!(parsed.codec, *codec, "{name}");
        assert_eq!(parsed.audio, *audio, "{name}");
        assert_eq!(parsed.hdr, *hdr, "{name}");
    }
}

#[test]
fn edition_tags() {
    let cases = [
        ("Directors.Cut", Some("Directors Cut")),
        ("Extended.Edition", Some("Extended Edition")),
        ("EXTENDED", Some("EXTENDED")),
        ("Theatrical.Cut", Some("Theatrical Cut")),
        (
            "Special.Edition.Remastered",
            Some("Special Edition Remastered"),
        ),
        ("25th.Anniversary.Edition", Some("25th Anniversary Edition")),
        ("UNRATED", Some("UNRATED")),
        ("Open.Matte", Some("Open Matte")),
        ("1080p", None),
    ];
    for (tags, edition) in cases {
        let name = format!("Some.Movie.2020.{tags}.BluRay.x264-GROUP");
        assert_eq!(parse(&name).edition.as_deref(), edition, "{name}");
    }
    // Without a year the edition ends the title.
    let parsed = parse("Some.Movie.Extended.1080p.BluRay.x264-GROUP");
    assert_eq!(parsed.title, "Some Movie");
    assert_eq!(parsed.edition.as_deref(), Some("Extended"));
}

#[test]
fn revision_tags() {
    // (tags, proper, repack, version)
    let cases = [
        ("1080p", false, false, 1),
        ("PROPER", true, false, 2),
        ("REPACK", false, true, 2),
        ("RERIP", false, true, 2),
        ("REPACK2", false, true, 3),
        ("PROPER.REPACK", true, true, 2),
    ];
    for (tags, proper, repack, version) in cases {
        let name = format!("Some.Movie.2020.{tags}.BluRay.x264-GROUP");
        let parsed = parse(&name);
        assert_eq!(
            (parsed.proper, parsed.repack, parsed.version),
            (proper, repack, version),
            "{name}"
        );
    }
}

#[test]
fn language_and_flag_tags() {
    let parsed = parse("Some.Movie.2020.MULTI.TRUEFRENCH.FRENCH.German.3D.HC.1080p.BluRay-GROUP");
    assert_eq!(
        parsed.languages,
        [Language::Multi, Language::French, Language::German]
    );
    assert!(parsed.three_d && parsed.hardcoded_subs);

    // A language word in the title is not a tag.
    let parsed = parse("The.French.Dispatch.2021.1080p.BluRay.x264-GROUP");
    assert_eq!(parsed.title, "The French Dispatch");
    assert!(parsed.languages.is_empty());
    assert!(!parsed.three_d && !parsed.hardcoded_subs);
}

#[test]
fn odd_inputs_do_not_panic() {
    let long = "A.".repeat(50_000) + "2019.1080p.BluRay.x264-GROUP";
    let odd = [
        "",
        " ",
        "...",
        "-_. -_.",
        "-",
        "[]",
        "()",
        "[[[",
        "2019",
        "1080p",
        "-GROUP",
        "Амели.2001.1080p.BluRay.x264-ГРУППА",
        "千と千尋の神隠し (2001) [1080p]",
        "Amélie_2001_1080p\u{0301}🎬-é",
        "\0\n\t\r",
        long.as_str(),
    ];
    for name in odd {
        let parsed = parse(name);
        if parsed.source == Source::Unknown {
            assert_eq!(quality(&parsed), "Unknown", "{name:?}");
        }
    }
    assert_eq!(parse("").title, "");
    assert_eq!(parse("Амели.2001.1080p.BluRay.x264-GROUP").title, "Амели");
    assert_eq!(parse(&long).year, Some(2019));
}

/// Fuzz without a dependency: random mixes of tokens, separators and odd
/// characters from a fixed-seed generator. Every result must be a quality
/// of docs/07 and no input may panic.
#[test]
fn random_names_never_panic() {
    const QUALITIES: &str = "Unknown, WORKPRINT, CAM, TELESYNC, TELECINE, REGIONAL, DVDSCR, \
        SDTV, DVD, DVD-R, WEBDL-480p, WEBRip-480p, Bluray-480p, Bluray-576p, HDTV-720p, \
        WEBDL-720p, WEBRip-720p, Bluray-720p, HDTV-1080p, WEBDL-1080p, WEBRip-1080p, \
        Bluray-1080p, Remux-1080p, HDTV-2160p, WEBDL-2160p, WEBRip-2160p, Bluray-2160p, \
        Remux-2160p, BR-DISK, Raw-HD";
    const PARTS: &[&str] = &[
        "Some",
        "Movie",
        "2019",
        "1968",
        "1080p",
        "2160p",
        "720p",
        "BluRay",
        "WEB-DL",
        "WEB",
        "REMUX",
        "COMPLETE",
        "HDTV",
        "TS",
        "CAM",
        "DVD-R",
        "x264",
        "H.265",
        "DTS-HD.MA",
        "DD+",
        "HDR10+",
        "DV",
        "IMAX",
        "The.Final.Cut",
        "PROPER",
        "REPACK2",
        "FRENCH",
        "-GROUP",
        "[YTS.MX]",
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
        "\u{0301}",
        "www.",
        ".mkv",
        "",
    ];
    let mut state: u64 = 53;
    let mut next = |bound: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as usize % bound
    };
    for _ in 0..5_000 {
        let name: String = (0..next(12)).map(|_| PARTS[next(PARTS.len())]).collect();
        let parsed = parse(&name);
        let quality = quality(&parsed);
        assert!(QUALITIES.split(", ").any(|q| q == quality), "{name:?}");
        assert!(parsed.version >= 1, "{name:?}");
    }
}

#[test]
fn every_language_has_a_unique_code() {
    let all = [
        (Language::English, "en"),
        (Language::French, "fr"),
        (Language::German, "de"),
        (Language::Spanish, "es"),
        (Language::Italian, "it"),
        (Language::Dutch, "nl"),
        (Language::Portuguese, "pt"),
        (Language::Russian, "ru"),
        (Language::Japanese, "ja"),
        (Language::Korean, "ko"),
        (Language::Chinese, "zh"),
        (Language::Swedish, "sv"),
        (Language::Norwegian, "no"),
        (Language::Danish, "da"),
        (Language::Finnish, "fi"),
        (Language::Polish, "pl"),
        (Language::Hindi, "hi"),
        (Language::Multi, "multi"),
    ];
    for (lang, expected) in all {
        assert_eq!(lang.code(), expected);
    }
    let unique: std::collections::HashSet<_> = all.iter().map(|(lang, _)| lang.code()).collect();
    assert_eq!(unique.len(), all.len(), "two languages share a code");
}
