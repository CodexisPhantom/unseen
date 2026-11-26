# 07 Release parsing and scoring

Status: reviewed (2026-11-16). Movies, with the TV rules in their own section
below (M7).

## Parsed fields

| Field | Example values |
|---|---|
| `title` | `The Matrix` |
| `year` | `1999` |
| `resolution` | 480p, 576p, 720p, 1080p, 2160p |
| `source` | CAM, TELESYNC, TELECINE, DVDSCR, DVD, HDTV, WEBDL, WEBRip, Bluray, Remux, BR-DISK |
| `quality` | Derived from source and resolution, a Radarr quality name |
| `codec` | x264, x265, AV1, XviD |
| `audio` | AAC, AC3, EAC3, DTS, DTS-HD MA, TrueHD, Atmos |
| `hdr` | HDR10, HDR10+, DV |
| `release_group` | `SPARKS` |
| `edition` | Director's Cut, Extended, IMAX, Remastered |
| `proper`, `repack`, `version` | revision of the same release |
| `languages` | From tags such as `FRENCH`, `MULTI`; default: unknown |
| `hardcoded_subs`, `3d` | flags |

## Qualities

Same names and order as Radarr, lowest to highest. Verify this list against
Radarr's `GET /api/v3/qualitydefinition` before implementing.

```
Unknown, WORKPRINT, CAM, TELESYNC, TELECINE, REGIONAL, DVDSCR, SDTV, DVD,
DVD-R, WEBDL-480p, WEBRip-480p, Bluray-480p, Bluray-576p, HDTV-720p,
WEBDL-720p, WEBRip-720p, Bluray-720p, HDTV-1080p, WEBDL-1080p, WEBRip-1080p,
Bluray-1080p, Remux-1080p, HDTV-2160p, WEBDL-2160p, WEBRip-2160p,
Bluray-2160p, Remux-2160p, BR-DISK, Raw-HD
```

Quality ids match Radarr's ids so the compat API can return them unchanged.

## Quality definitions (size limits)

As in Radarr: each quality has a minimum, a preferred and a maximum size in
megabytes per minute of runtime. The table is seeded with Radarr's defaults
and editable (`profiles:manage`). Instance import and the TRaSH guide's
quality sizes can overwrite it. A release whose size is outside minimum and
maximum is rejected. A movie without a known runtime skips the check.

The seed is the answer of `GET /api/v3/qualitydefinition` from a new Radarr
7.0.3, recorded in `fixtures/radarr/qualitydefinition.json`; a test compares
every row with it. Radarr gives no preferred size and no maximum from
Bluray-1080p up (no limit). Sizes are megabytes of 1024 x 1024 bytes per
minute. The check lives in `core` (`SizeLimits::check`); using it in
rejection and scoring is a separate issue. The API validates a change: the
minimum is 0 or more, the maximum is above 0 and not below the minimum, and
the preferred size lies between them.

## Quality profile

- An ordered list of qualities or quality groups, each allowed or not.
- `cutoff`: the quality at which upgrades stop.
- `upgradeAllowed`: whether a better release replaces an existing file.
- `language`: `any`, `original`, or one language, as in Radarr. Default
  `any`. Finer language preferences are expressed with language conditions
  in custom formats, which is how the TRaSH guides do it.
- Custom format scores: one score per custom format, default 0.
- `minFormatScore`: releases with a lower total score are rejected.
- `cutoffFormatScore`: upgrades for score stop when the file reaches it.
- `minUpgradeFormatScore`: smallest score gain that counts as an upgrade.

## Custom formats

Same model and JSON format as Radarr, so that formats exported from Radarr
and published by the TRaSH guides load unchanged. `fields` deserialize from
both Radarr's list form (`[{"name": "value", "value": 1080}]`) and the TRaSH
guide map form (`{"value": 1080}`), and serialize in the list form.

- A custom format is a name and a list of conditions (specifications).
- Each condition has a type, a value, `negate` and `required`.
- Matching is Radarr's rule: every required condition must match, and
  conditions are grouped by their `implementation` string. Each group needs
  at least one match, and a matching required condition counts toward its
  group, so a group of required conditions must have all of them match. A
  format with no conditions matches every release. `negate` flips one
  condition's own result before grouping.
- Condition types in V1: release title (regex), release group (regex),
  edition (regex), language, source, resolution, quality modifier (remux and
  others), size, indexer flag, year. Regexes use `fancy-regex`, because the
  guide data uses look-ahead and look-behind. A pattern matches
  case-insensitively and unanchored; an empty pattern matches nothing, and a
  pattern that does not compile does not match and does not panic.
- The total score of a release for a profile is the sum of the scores of the
  matching formats.
- Formats with `includeWhenRenaming` appear in the `{Custom Formats}` naming
  token.
- A test endpoint takes a release name and returns the matching formats and
  the score per profile.

## TRaSH guides

Unseen reads the JSON that the TRaSH guides publish in their repository
(custom formats, custom format groups, quality profiles with score sets,
quality sizes for Radarr). Verify the paths and file formats when the issue
is implemented.

- A scheduled task downloads the guide data and stores it locally.
- The guide browser lists everything with its description, searchable by
  name and by purpose (for example "HDR", "unwanted", "French audio").
- Applying shows a preview first: which custom formats are created or
  changed, which scores change in which profile, which quality sizes change.
- An applied item can be kept in sync. When the guide changes, the task
  updates synced items and reports what changed. Items the user edited are
  not overwritten; the user sees the difference and chooses.
- The data and its licence stay with the TRaSH guides project; the UI links
  to the guide page of each item.

## Rejection rules (in order)

1. The title or year does not match the movie.
2. Quality is not allowed in the profile.
3. The release is on the blocklist.
4. Size is outside the quality definition limits.
5. Language does not satisfy the profile language. A release name without a
   language tag counts as the movie's original language (verify against
   Radarr).
6. The custom format score is below `minFormatScore`.
7. The movie profile already has a file and upgrades are off, or both the
   quality cutoff and the format score cutoff are met.
8. The release is not better than the existing file.
9. The indexer has tags and shares none with the movie.

A rejected release is still listed in manual search, with its reasons.

## Scoring (accepted releases)

Compare in this order, first difference wins:

1. Quality rank in the profile.
2. Custom format score.
3. Revision: repack/proper version, higher wins.
4. Indexer priority.
5. Seeders (torrents), on a log scale.
6. Size, closest to the preferred size of the quality definition; larger
   wins when no preferred size is set.

Verify this order against Radarr's decision comparer before implementing.

## Upgrade rules

A release upgrades the file of a movie profile when `upgradeAllowed` is on
and one of these is true:

- the existing quality is below the cutoff and the release has a higher
  quality rank;
- the quality is the same, the existing format score is below
  `cutoffFormatScore`, and the release scores at least
  `minUpgradeFormatScore` more;
- the quality and score are the same and the release is a higher revision.

## Implementation notes

The rules live in `crates/core/src/scoring.rs`. Rules 3 and 6 (blocklist and
format score) and the custom format step of the comparison are not there yet:
they arrive with the blocklist and custom formats. Choices the doc left open:

- `rank` lists every rule a release breaks, in the order above, and sorts
  accepted releases before rejected ones. A rejected release is ordered like
  an accepted one, so manual search stays stable.
- Releases are compared by their position in the profile; the members of a
  quality group are equal. Indexer priority is 1 to 50 and a lower number
  wins. Seeders are rounded on a `log10(seeders + 1)` scale. The title and the
  guid end every tie, so the order does not depend on the input order.
- A release is equal to the existing file when its position in the profile
  and its revision are the same. It is not an upgrade. When the existing file
  is at or above the cutoff, only a higher revision of the same quality
  replaces it; every other release is rejected with "cutoff met".
- Title match ignores case and punctuation. A release without a year matches;
  a release with another year does not.
- A release with no language tag counts as the original language of the
  movie. A `MULTI` release satisfies any language. A profile language code
  without a matching release tag in the parser cannot be satisfied by a
  tagged release.
- A quality without size limits, a release without a size and a movie without
  a runtime skip the size check.
- Usenet (M9): `Candidate` accepts either a torrent (`torznab::Release`) or a
  Usenet release (`newznab::NzbRelease`) via `ReleaseRef`. `ProtocolPreference`
  (`None`, `PreferUsenet`, `PreferTorrent`, `OnlyUsenet`, `OnlyTorrent`) can be
  resolved per profile with a global fallback via `ProtocolPreference::resolve`.
  - Rejection: `OnlyUsenet` / `OnlyTorrent` reject the other protocol with
    `protocol_not_allowed`. When `retention_days` is set (non-zero) and `now`
    is supplied, a Usenet release whose age in whole days (from `usenetdate`,
    falling back to `pubDate`) exceeds `retention_days` is rejected with
    `retention_exceeded`; torrents and releases without a timestamp skip the
    retention check.
  - Scoring: protocol preference breaks ties right after revision and before
    indexer priority. For two Usenet releases, age in hours on a
    `log10(age_hours + 1)` scale (smaller/newer wins; known age beats unknown)
    replaces seeders, and releases in the same bucket fall through to size.

## Test table

These rows become unit tests. Expected values are a draft. Before
implementing, run each name through a real Radarr
(`GET /api/v3/parse?title=...`) and replace the row with what Radarr reports.

| Release name | Title | Year | Quality | Group | Notes |
|---|---|---|---|---|---|
| `The.Matrix.1999.1080p.BluRay.x264-SPARKS` | The Matrix | 1999 | Bluray-1080p | SPARKS | |
| `The.Matrix.1999.2160p.UHD.BluRay.REMUX.HDR.HEVC.TrueHD.7.1.Atmos-FGT` | The Matrix | 1999 | Remux-2160p | FGT | HDR10, Atmos |
| `Dune.Part.Two.2024.2160p.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-FLUX` | Dune Part Two | 2024 | WEBDL-2160p | FLUX | DV, HDR10 |
| `Dune.Part.Two.2024.1080p.WEBRip.x265.10bit.AAC5.1-GROUP` | Dune Part Two | 2024 | WEBRip-1080p | GROUP | |
| `Oppenheimer.2023.IMAX.1080p.BluRay.REMUX.AVC.DTS-HD.MA.5.1-TRiToN` | Oppenheimer | 2023 | Remux-1080p | TRiToN | edition IMAX |
| `Blade.Runner.1982.The.Final.Cut.720p.BluRay.x264-AMIABLE` | Blade Runner | 1982 | Bluray-720p | AMIABLE | edition The Final Cut |
| `Some.Movie.2023.REPACK.1080p.WEB.H264-NAISU` | Some Movie | 2023 | WEBDL-1080p | NAISU | repack, bare `WEB` |
| `Some.Movie.2023.PROPER.720p.HDTV.x264-GROUP` | Some Movie | 2023 | HDTV-720p | GROUP | proper |
| `Some.Movie.2024.HDCAM.x264-GROUP` | Some Movie | 2024 | CAM | GROUP | |
| `Some.Movie.2024.TS.XviD-GROUP` | Some Movie | 2024 | TELESYNC | GROUP | |
| `Some.Movie.2010.DVDRip.XviD-GROUP` | Some Movie | 2010 | DVD | GROUP | |
| `Some.Movie.2015.COMPLETE.BLURAY-GROUP` | Some Movie | 2015 | BR-DISK | GROUP | |
| `Le.Fabuleux.Destin.d.Amelie.Poulain.2001.FRENCH.1080p.BluRay.x264-GROUP` | Le Fabuleux Destin d Amelie Poulain | 2001 | Bluray-1080p | GROUP | language French |
| `2001.A.Space.Odyssey.1968.1080p.BluRay.x264-GROUP` | 2001 A Space Odyssey | 1968 | Bluray-1080p | GROUP | year-like title |
| `Some Movie (2019) [1080p] [BluRay] [YTS.MX]` | Some Movie | 2019 | Bluray-1080p | YTS.MX | bracket style |
| `Some.Movie.2019.1080p.x264-GROUP` | Some Movie | 2019 | Unknown | GROUP | resolution without source; check what Radarr reports |

## TV (M7)

Release names for shows. Everything above applies unless this section
changes it. Expected values are drafts: before implementing (#90), run each
name through a real Sonarr (`GET /api/v3/parse?title=...`) and replace the
row with what Sonarr reports.

### Parsed fields for TV

These are added to the fields of the first table. `edition` does not apply.
A year in the name is the show's year; it tells shows with the same title
apart.

| Field | Example values |
|---|---|
| `title` | `The Expanse`, `Doctor Who` (a year in the name goes to `year`) |
| `season` | `3`; `0` for the specials; a list for a multi-season pack |
| `episodes` | `[4]`, `[4, 5]`, `[4, 5, 6]` |
| `absolute_episodes` | `[112]`, `[5, 6, 7, 8]` (anime) |
| `air_date` | `2024-05-17` (daily) |
| `release_type` | `episode`, `multi_episode`, `season_pack`, `multi_season_pack` |
| `version` | `2` for `v2` in an anime name; same role as `proper` and `repack` |
| `hash` | `ABCD1234`, the CRC32 that anime groups put in brackets |

Forms the parser reads:

| Form | Example | Result |
|---|---|---|
| Season and episode | `S03E04`, `3x04` | season 3, episode 4 |
| Several episodes in one release | `S03E04E05`, `S03E04-E06`, `S03E04-06` | season 3, episodes 4 to 5 or 4 to 6, `multi_episode` |
| Season pack | `S03`, `Season.3`, `S03.COMPLETE` | season 3, no episodes, `season_pack` |
| Several seasons | `S01-S03` | `multi_season_pack` |
| Special | `S00E01` | season 0, episode 1 |
| Air date | `2024.05.17`, `2024-05-17` | `air_date` |
| Anime, absolute | `[Group] Show - 112 [1080p]` | `absolute_episodes` 112 |
| Anime, revision | `[Group] Show - 112v2 [1080p]` | absolute 112, `version` 2 |
| Anime, range or batch | `[Group] Show - 05-08`, `[Group] Show (01-28) [BD 1080p]` | absolute range; a batch is a `season_pack` over the episodes in the range |

Matching a parsed name to the show's episodes:

1. Find the show by its normalised title (lower case, no punctuation, no
   leading "the", no year) among its title and its aliases. A year in the name
   must equal the show's year. This is the movie rule, with aliases.
2. Find the episodes by the series type: season and episode number for
   `standard` (and for any show when the name has them), air date for `daily`,
   absolute number for `anime`. A name that matches no episode is rejected
   with the reason.
3. A name with a season and no episode is a season pack of that season.

### Qualities for TV

One `Quality` enum in `core` serves movies and shows, with the ids of
Radarr, which is what the V1 tables store. A show profile may allow only
these qualities: `Unknown`, `SDTV`, `DVD`, `WEBDL-480p`, `WEBRip-480p`,
`Bluray-480p`, `Bluray-576p`, `HDTV-720p`, `WEBDL-720p`, `WEBRip-720p`,
`Bluray-720p`, `HDTV-1080p`, `WEBDL-1080p`, `WEBRip-1080p`, `Bluray-1080p`,
`Remux-1080p`, `HDTV-2160p`, `WEBDL-2160p`, `WEBRip-2160p`, `Bluray-2160p`,
`Remux-2160p`, `Raw-HD`. The movie-only qualities (`WORKPRINT`, `CAM`,
`TELESYNC`, `TELECINE`, `REGIONAL`, `DVDSCR`, `DVD-R`, `BR-DISK`) are never
parsed from a TV name and never offered in a show profile.

Default order of a new show profile, lowest to highest, as Sonarr's default
quality definitions weigh them (a slash joins qualities of equal weight, which
Sonarr groups): `Unknown`, `SDTV`, `WEBRip-480p` / `WEBDL-480p`, `DVD`,
`Bluray-480p`, `Bluray-576p`, `HDTV-720p`, `HDTV-1080p`, `Raw-HD`,
`WEBRip-720p` / `WEBDL-720p`, `Bluray-720p`, `WEBRip-1080p` / `WEBDL-1080p`,
`Bluray-1080p`, `Remux-1080p`, `HDTV-2160p`, `WEBRip-2160p` / `WEBDL-2160p`,
`Bluray-2160p`, `Remux-2160p`. The order differs from the movie order above
(for example `HDTV-1080p` is above `Bluray-720p` there and below it here).
Source: `Quality.cs` in Sonarr, read on 2026-11-25. Check it against
`GET /api/v3/qualitydefinition` of a new Sonarr, as for movies.

Quality ids: Sonarr and Radarr give the same id to every quality they share,
except four. Unseen stores Radarr's ids. The Sonarr compat layer translates
these four in both directions:

| Quality (Sonarr's name) | Unseen and Radarr id | Sonarr id |
|---|---|---|
| `Bluray-480p` | 20 | 13 |
| `Bluray-576p` | 21 | 22 |
| `Remux-1080p` (`Bluray-1080p Remux`) | 30 | 20 |
| `Remux-2160p` (`Bluray-2160p Remux`) | 31 | 21 |

Both tables come from the two `Quality.cs` files read on 2026-11-25. The
translation table lives in `arr-compat`, and a test compares it with
`fixtures/sonarr/qualitydefinition.json`.

### Quality definitions for TV

The `show` rows of `quality_definitions` are seeded with the answer of
`GET /api/v3/qualitydefinition` from a new Sonarr, recorded in
`fixtures/sonarr/qualitydefinition.json`; a test compares every row with it.
Units and validation are as for movies (megabytes per minute, editable with
`profiles:manage`).

The size of a release is divided by the runtime of the episodes it covers:
`size / (sum of runtimes)`. For a pack that is the average per episode, which
is Sonarr's rule. The runtime of an episode is its own, else the show's; a
covered episode without any runtime skips the check.

### Custom formats for TV

Custom formats of kind `show` use the same model and JSON. The conditions of
Sonarr are: release title, release group, language, source, resolution, size,
indexer flag and release type (`episode`, `multi_episode`, `season_pack`).
Edition, year and quality modifier are movie conditions and do not exist for
shows. A profile scores only formats of its own kind.

### Rejection rules for TV (in order)

Replaces the list for movies when the target is an episode.

1. The title does not match the show or an alias, or the year differs.
2. The name matches no episode of the show (numbering, air date or absolute
   number). The reason names which.
3. The release is a multi-season pack (grabbed by hand only), or a split
   episode such as `S01E01A`.
4. A covered episode is not a wanted episode of the profile: unmonitored,
   not aired, or already has a file the release would not upgrade. This is
   why a pack for a season with unwanted episodes is rejected. Sonarr
   accepts such a pack when it upgrades enough episodes; Unseen takes the
   simpler rule and downloads those episodes one by one.
5. A covered episode is in an active download of the same show profile.
6. Quality is not allowed in the profile.
7. The release is on the blocklist of the show.
8. Size is outside the quality definition limits (rule above).
9. Language does not satisfy the profile language.
10. The custom format score is below `minFormatScore`.
11. The profile has a file for a covered episode and upgrades are off, or
    both cutoffs are met.
12. The release is not better than the files it replaces, or a file on disk
    covers more episodes than the release (a single episode never replaces a
    multi-episode file).
13. The indexer has tags and shares none with the show.

A rejected release is still listed in manual search, with its reasons. A
hand grab may override rules 3 and 4 (a multi-season pack, a pack that covers
unwanted episodes); it never overrides rule 5, because two active downloads
of one profile cannot cover the same episode.

### Season search

For each show profile and season, the missing search does this:

1. When every episode of the season is a wanted episode of the profile, search
   for the season and take the best accepted release. A pack wins over
   several single-episode releases of equal rank.
2. Otherwise, or when step 1 finds nothing acceptable, search for each wanted
   episode on its own.

Scoring uses the order for movies, with one step added after the revision:
in a season search the release that covers more of the wanted episodes wins;
in an episode search the release that covers fewer wins. Verify against
Sonarr's decision comparer. A revision (`proper`, `repack`, anime `v2`) counts
as for movies.

### Upgrade rules for TV

The upgrade rules for movies apply per episode and per show profile. A release
that covers several episodes is an upgrade only when it upgrades every one of
them (or fills a missing one). After an import, a file that no longer covers
any episode of its profile is deleted; a multi-episode file that still covers
another episode stays.

### TV test table

Expected values verified against Sonarr's parsing rules (`Parser.cs`,
`QualityParser.cs`, `ReleaseGroupParser.cs` and `Quality.cs` on Sonarr
`develop`). In Sonarr, a resolution without a source tag (such as `(1080p)` or
`[720p]` in anime releases) maps to `HDTV-1080p` / `HDTV-720p` (or `SDTV` for
480p/540p/576p). Every row is asserted by `crates/core/src/tv_release/tests.rs`.

| Release name | Title | Numbering | Quality | Group | Notes |
|---|---|---|---|---|---|
| `The.Expanse.S03E04.1080p.WEB-DL.DD5.1.H.264-GROUP` | The Expanse | S03E04 | WEBDL-1080p | GROUP | |
| `The.Expanse.S03E04E05.1080p.WEB-DL.DD5.1.H.264-GROUP` | The Expanse | S03E04, E05 | WEBDL-1080p | GROUP | multi-episode |
| `The.Expanse.S03E04-E06.720p.HDTV.x264-GROUP` | The Expanse | S03E04 to E06 | HDTV-720p | GROUP | multi-episode, range |
| `The.Expanse.S03E04-06.720p.HDTV.x264-GROUP` | The Expanse | S03E04 to E06 | HDTV-720p | GROUP | multi-episode, short range |
| `The.Expanse.3x04.720p.HDTV.x264-GROUP` | The Expanse | S03E04 | HDTV-720p | GROUP | `3x04` form |
| `The.Expanse.S03E04.PROPER.1080p.WEB.H264-GROUP` | The Expanse | S03E04 | WEBDL-1080p | GROUP | proper, bare `WEB` |
| `The.Expanse.S03E04.REPACK.2160p.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-GROUP` | The Expanse | S03E04 | WEBDL-2160p | GROUP | repack, DV, HDR10, Atmos |
| `The.Expanse.S03E04.1080p.BluRay.x264-GROUP` | The Expanse | S03E04 | Bluray-1080p | GROUP | |
| `The.Expanse.S03E04.2160p.UHD.BluRay.REMUX.HDR.HEVC-GROUP` | The Expanse | S03E04 | Remux-2160p | GROUP | HDR10, x265, Sonarr calls it `Bluray-2160p Remux` |
| `The.Expanse.S03E04.1080p.BluRay.REMUX.AVC.DTS-HD.MA.5.1-GROUP` | The Expanse | S03E04 | Remux-1080p | GROUP | x264, Sonarr calls it `Bluray-1080p Remux` |
| `The.Expanse.S03E04.HDTV.x264-GROUP` | The Expanse | S03E04 | SDTV | GROUP | |
| `The.Expanse.S03E04.DVDRip.XviD-GROUP` | The Expanse | S03E04 | DVD | GROUP | |
| `The.Expanse.S03E04.WEBRip.x264-GROUP` | The Expanse | S03E04 | WEBRip-480p | GROUP | no resolution |
| `The.Expanse.S03E04.WEB-DL.x264-GROUP` | The Expanse | S03E04 | WEBDL-480p | GROUP | no resolution |
| `The.Expanse.S03E04.480p.BluRay.x264-GROUP` | The Expanse | S03E04 | Bluray-480p | GROUP | |
| `The.Expanse.S03E04.576p.BluRay.x264-GROUP` | The Expanse | S03E04 | Bluray-576p | GROUP | |
| `The.Expanse.S03E04.720p.BluRay.x264-GROUP` | The Expanse | S03E04 | Bluray-720p | GROUP | |
| `The.Expanse.S03E04.2160p.UHD.BluRay.x265-GROUP` | The Expanse | S03E04 | Bluray-2160p | GROUP | x265 |
| `The.Expanse.S03E04.720p.WEBRip.x264-GROUP` | The Expanse | S03E04 | WEBRip-720p | GROUP | |
| `The.Expanse.S03E04.1080p.AMZN.WEBRip.DDP5.1.x264-NTb` | The Expanse | S03E04 | WEBRip-1080p | NTb | |
| `The.Expanse.S03E04.2160p.NF.WEBRip.DDP5.1.x265-FLUX` | The Expanse | S03E04 | WEBRip-2160p | FLUX | x265 |
| `The.Expanse.S03E04.1080p.HDTV.x264-GROUP` | The Expanse | S03E04 | HDTV-1080p | GROUP | |
| `The.Expanse.S03E04.2160p.HDTV.x265-GROUP` | The Expanse | S03E04 | HDTV-2160p | GROUP | x265 |
| `The.Expanse.S03E04.1080i.HDTV.DD5.1.MPEG2-TrollHD` | The Expanse | S03E04 | Raw-HD | TrollHD | MPEG2 HDTV |
| `The.Expanse.S03E04.Raw-HD.ts` | The Expanse | S03E04 | Raw-HD | | Raw-HD tag |
| `The.Expanse.S03.E04.1080p.WEB-DL.x264-GROUP` | The Expanse | S03E04 | WEBDL-1080p | GROUP | dot between season and episode |
| `The Expanse - S03 - E04 - 720p.HDTV.x264-GROUP` | The Expanse | S03E04 | HDTV-720p | GROUP | separated season and episode |
| `The.Expanse.S03.Ep.04.1080p.BluRay.x264-GROUP` | The Expanse | S03E04 | Bluray-1080p | GROUP | `Ep.` form |
| `The.Expanse.Season.3.Episode.04.1080p.WEB-DL.x264-GROUP` | The Expanse | S03E04 | WEBDL-1080p | GROUP | `Season Episode` form |
| `The.Expanse.S03.04.1080p.BDRip.x264-GROUP` | The Expanse | S03E04 | Bluray-1080p | GROUP | `S03.04` form |
| `The.Expanse.103.720p.HDTV.x264-DIMENSION` | The Expanse | S01E03 | HDTV-720p | DIMENSION | SEE numbering |
| `The.Expanse.1013.720p.HDTV.x264-DIMENSION` | The Expanse | S10E13 | HDTV-720p | DIMENSION | SSEE numbering |
| `The.Expanse.S2016E14.720p.HDTV.x264-GROUP` | The Expanse | S2016E14 | HDTV-720p | GROUP | 4-digit year season |
| `The.Expanse.S03E115.DVDRip.XviD-OSiTV` | The Expanse | S03E115 | DVD | OSiTV | 3-digit episode |
| `S03E09.720p.HDTV.x264-GROUP` | | S03E09 | HDTV-720p | GROUP | no title |
| `The.Expanse.S03E04.S03E05.720p.HDTV.x264-DIMENSION` | The Expanse | S03E04, E05 | HDTV-720p | DIMENSION | multi-episode, repeated SxxExx |
| `The.Expanse.S03E04.E05.1080p.WEB-DL.x264-GROUP` | The Expanse | S03E04, E05 | WEBDL-1080p | GROUP | multi-episode, separated E |
| `The.Expanse.S03E04-05-06.1080p.BluRay.x264-GROUP` | The Expanse | S03E04 to E06 | Bluray-1080p | GROUP | multi-episode, triple dash |
| `The.Expanse.2x04.2x05.720p.BluRay-FUTV` | The Expanse | S02E04, E05 | Bluray-720p | FUTV | multi-episode, repeated NxEE |
| `The.Expanse.2x04-06.720p.HDTV.x264-GROUP` | The Expanse | S02E04 to E06 | HDTV-720p | GROUP | multi-episode, NxEE range |
| `The.Expanse.S01E99-100.1080p.WEB-DL.x264-GROUP` | The Expanse | S01E99, E100 | WEBDL-1080p | GROUP | multi-episode, 2 to 3 digits |
| `The.Expanse.Season.01.Episode.05-06.720p.WEB-DL.x264-GROUP` | The Expanse | S01E05, E06 | WEBDL-720p | GROUP | multi-episode, word range |
| `The.Expanse.S01E01A.1080p.WEB-DL.x264-GROUP` | The Expanse | S01E01 | WEBDL-1080p | GROUP | split episode |
| `The.Expanse.S06E01b.Fade.Out.1080p.DSNP.WEB-DL.AAC2.0.H.264-FLUX` | The Expanse | S06E01 | WEBDL-1080p | FLUX | split episode |
| `The.Expanse.S01E01.Part.1.1080p.WEB-DL.x264-GROUP` | The Expanse | S01E01 | WEBDL-1080p | GROUP | split episode, part 1 |
| `The.Expanse.S01E01.Part2.720p.HDTV.x264-GROUP` | The Expanse | S01E01 | HDTV-720p | GROUP | split episode, part 2 |
| `Doctor.Who.2005.S01E01.1080p.WEB-DL.x264-GROUP` | Doctor Who | S01E01 | WEBDL-1080p | GROUP | year 2005 |
| `Doctor.Who.(2005).S01E01.1080p.BluRay.x264-GROUP` | Doctor Who | S01E01 | Bluray-1080p | GROUP | year 2005, parenthesised year |
| `24.S01E01.720p.HDTV.x264-GROUP` | 24 | S01E01 | HDTV-720p | GROUP | numeric title |
| `1923.S01E01.1080p.WEB-DL.x264-GROUP` | 1923 | S01E01 | WEBDL-1080p | GROUP | year-like numeric title |
| `1923.2022.S01E01.1080p.WEB-DL.x264-GROUP` | 1923 | S01E01 | WEBDL-1080p | GROUP | year 2022, numeric title with year |
| `Shameless.US.S02E03.1080p.WEB-DL.x264-GROUP` | Shameless US | S02E03 | WEBDL-1080p | GROUP | country suffix |
| `Brooklyn.Nine-Nine.S01E01.1080p.BluRay.x264-GROUP` | Brooklyn Nine-Nine | S01E01 | Bluray-1080p | GROUP | hyphenated title |
| `The.Expanse.S00E01.1080p.WEB-DL.x264-GROUP` | The Expanse | S00E01 | WEBDL-1080p | GROUP | special |
| `The.Expanse.S00E01E02.1080p.BluRay.x264-GROUP` | The Expanse | S00E01, E02 | Bluray-1080p | GROUP | special, multi-episode |
| `[Impatience] Show - 0x01 [720p][34073169].mkv` | Show | S00E01 | HDTV-720p | Impatience | special, hash 34073169 |
| `Series Title S01E11.5 [SP] [1080p].mkv` | Series Title | S01E11 | HDTV-1080p | | special, decimal SP |
| `[DeadFish] Frieren - 01 - OVA [BD][720p][AAC]` | Frieren | absolute 1 | Bluray-720p | DeadFish | special, OVA |
| `The.Expanse.S03.1080p.BluRay.x264-GROUP` | The Expanse | S03 | Bluray-1080p | GROUP | season pack |
| `The.Expanse.Season.3.1080p.BluRay.x264-GROUP` | The Expanse | S03 | Bluray-1080p | GROUP | season pack, `Season.3` form |
| `The.Expanse.S03.COMPLETE.720p.WEB-DL.x264-GROUP` | The Expanse | S03 | WEBDL-720p | GROUP | season pack |
| `The.Expanse.Saison.3.FRENCH.720p.HDTV.x264-GROUP` | The Expanse | S03 | HDTV-720p | GROUP | season pack, language French |
| `The.Expanse.Stagione.3.ITALIAN.720p.HDTV.x264-GROUP` | The Expanse | S03 | HDTV-720p | GROUP | season pack, language Italian |
| `The.Expanse.S2014.720p.HDTV.x264-ME` | The Expanse | S2014 | HDTV-720p | ME | season pack, 4-digit season |
| `[Vivid] Frieren S01 [Web][MKV][h264][1080p][AAC 2.0]` | Frieren | S01 | WEBDL-1080p | Vivid | season pack, anime S01 |
| `[Judas] Frieren (Season 1) [BD 1080p] [Batch]` | Frieren | S01 | Bluray-1080p | Judas | season pack, anime Season 1 |
| `The.Expanse.S01-S03.1080p.BluRay.x264-GROUP` | The Expanse | S01 to S03 | Bluray-1080p | GROUP | multi-season, hand grab only |
| `The.Expanse.S01-05.1080p.WEB-DL.x264-GROUP` | The Expanse | S01 to S05 | WEBDL-1080p | GROUP | multi-season, short range |
| `The.Expanse.Season.01-04.720p.BluRay.x264-GROUP` | The Expanse | S01 to S04 | Bluray-720p | GROUP | multi-season, word range |
| `The.Expanse.Season.01.-.Season.03.1080p.BluRay.x264-GROUP` | The Expanse | S01 to S03 | Bluray-1080p | GROUP | multi-season, repeated Season |
| `The.Expanse.S01.S02.S03.1080p.WEB-DL.x264-GROUP` | The Expanse | S01 to S03 | WEBDL-1080p | GROUP | multi-season, explicit list |
| `[Group] Frieren S01-S03 (BD 1080p)` | Frieren | S01 to S03 | Bluray-1080p | Group | multi-season, anime |
| `The.Expanse.S02.Part.1.1080p.NF.WEBRip.DD5.1.x264-NTb` | The Expanse | S02 | WEBRip-1080p | NTb | partial season part 1 |
| `The.Expanse.S07.Vol.1.1080p.BluRay.x264-GROUP` | The Expanse | S07 | Bluray-1080p | GROUP | partial season part 1 |
| `The.Expanse.S06.P2.1080p.BluRay.x265-GROUP` | The Expanse | S06 | Bluray-1080p | GROUP | partial season part 2 |
| `The.Daily.Show.2024.05.17.1080p.WEB.h264-GROUP` | The Daily Show | 2024-05-17 | WEBDL-1080p | GROUP | daily |
| `Late.Night.2024-05-17.720p.HDTV.x264-GROUP` | Late Night | 2024-05-17 | HDTV-720p | GROUP | daily, dashes |
| `The.Daily.Show.2024.05.17.Guest.Name.720p.HDTV.x264-GROUP` | The Daily Show | 2024-05-17 | HDTV-720p | GROUP | daily with a guest |
| `The.Daily.Show.2024.05.17.Part.2.720p.HULU.WEBRip.x264-GROUP` | The Daily Show | 2024-05-17 | WEBRip-720p | GROUP | daily, part 2 |
| `The.Daily.Show.20240517.720p.WEBRip.x264-GROUP` | The Daily Show | 2024-05-17 | WEBRip-720p | GROUP | daily, compact YYYYMMDD |
| `The.Daily.Show.140722.720p.HDTV.x264-YesTV` | The Daily Show | 2014-07-22 | HDTV-720p | YesTV | daily, 6-digit YYMMDD |
| `The.Daily.Show.30-04-2024.HDTV.1080p.H264-GROUP` | The Daily Show | 2024-04-30 | HDTV-1080p | GROUP | daily, DD-MM-YYYY |
| `The_Series_US_04.28.2014_hdtv.x264-GROUP.mp4` | The Series US | 2014-04-28 | SDTV | GROUP | daily, MM.DD.YYYY |
| `2020.A.Late.Talk.Show.2012.16.02.PDTV.XviD-C4TV` | 2020 A Late Talk Show | 2012-02-16 | SDTV | C4TV | daily, swapped month/day |
| `The.Daily.Show.5th.Mar.2025.1080p.WEB-DL.x264-GROUP` | The Daily Show | 2025-03-05 | WEBDL-1080p | GROUP | daily, short month |
| `[SubsPlease] Frieren - 12 (1080p) [ABCD1234].mkv` | Frieren | absolute 12 | HDTV-1080p | SubsPlease | hash ABCD1234, resolution without source |
| `[SubsPlease] Frieren - 12v2 (1080p) [ABCD1234].mkv` | Frieren | absolute 12 | HDTV-1080p | SubsPlease | version 2, hash ABCD1234 |
| `[SubsPlease] Frieren - 12v3 (1080p) [1234ABCD].mkv` | Frieren | absolute 12 | HDTV-1080p | SubsPlease | version 3, hash 1234ABCD |
| `[Group] One Piece - 1085 [1080p][HEVC]` | One Piece | absolute 1085 | HDTV-1080p | Group | x265, large absolute number |
| `[Group] Frieren - 05-08 [720p]` | Frieren | absolute 5 to 8 | HDTV-720p | Group | multi-episode, absolute range |
| `[Group] Frieren (01-28) [BD 1080p] [Batch]` | Frieren | absolute 1 to 28 | Bluray-1080p | Group | batch |
| `[Erai-raws] Jujutsu Kaisen - 01 ~ 12 [1080p]` | Jujutsu Kaisen | absolute 1 to 12 | HDTV-1080p | Erai-raws | multi-episode, tilde range |
| `[HatSubs] One Piece (1017-1088) (WEB 1080p)` | One Piece | absolute 1017 to 1088 | WEBDL-1080p | HatSubs | multi-episode, 4-digit range |
| `[BakeSubs] 86 - 01 [1080p][D40A9E55].mkv` | 86 | absolute 1 | HDTV-1080p | BakeSubs | hash D40A9E55, numeric anime title |
| `[HorribleSubs] Mob Psycho 100 S2 - 07 [720p].mkv` | Mob Psycho 100 S2 | absolute 7 | HDTV-720p | HorribleSubs | anime season in title |
| `[Shark-Raws] One Piece #957 (1280x720 x264 AAC).mp4` | One Piece | absolute 957 | HDTV-720p | Shark-Raws | `#` episode form |
| `[Exiled-Destiny] Bleach Ep01 (D2201EC5).mkv` | Bleach | absolute 1 | HDTV-720p | Exiled-Destiny | hash D2201EC5, extension fallback |
| `[Kaleido-subs] Frieren - 12 (S01E12) (WEB 1080p HEVC) [1ADD8F6D]` | Frieren | S01E12, absolute 12 | WEBDL-1080p | Kaleido-subs | hash 1ADD8F6D, x265, dual numbering |
| `[SubDESU]_Show_One_07_(1280x720_x264-AAC)_[6B7FD717]` | Show One | absolute 7 | HDTV-720p | SubDESU | hash 6B7FD717, underscores |
| `One.Piece.E1135.GERMAN.1080p.WEBRip.x264-GROUP` | One Piece | absolute 1135 | WEBRip-1080p | GROUP | language German, 4-digit E |
| `Frieren.S01E12.1080p.WEB-DL.AAC2.0.H.264-GROUP` | Frieren | S01E12 | WEBDL-1080p | GROUP | anime with season numbering |
| `Dix.pour.cent.S05E03.FRENCH.1080p.WEB.H264-GROUP` | Dix pour cent | S05E03 | WEBDL-1080p | GROUP | language French |
| `Some.Show.S01E01.MULTi.1080p.WEB.H264-GROUP` | Some Show | S01E01 | WEBDL-1080p | GROUP | language Multi |
| `Dark.S01E01.GERMAN.1080p.NF.WEB-DL.DDP5.1.H.264-GROUP` | Dark | S01E01 | WEBDL-1080p | GROUP | language German |
| `The.Expanse.S03E04.REPACK2.1080p.AMZN.WEB-DL.DDP5.1.H.264-NTb` | The Expanse | S03E04 | WEBDL-1080p | NTb | repack, version 3 |
| `The.Expanse.S03E04.RERIP.720p.BluRay.x264-DEMAND` | The Expanse | S03E04 | Bluray-720p | DEMAND | repack, version 2 |
| `The.Expanse.S03E04.1080p.x264-GROUP` | The Expanse | S03E04 | HDTV-1080p | GROUP | bare 1080p maps to HDTV-1080p |
| `The.Expanse.S03E04.480p.x264-GROUP` | The Expanse | S03E04 | SDTV | GROUP | bare 480p maps to SDTV |
| `The.Expanse.S03E04.x264-GROUP` | The Expanse | S03E04 | SDTV | GROUP | bare x264 maps to SDTV |
| `The.Expanse.S03E04.CAM.XviD-GROUP` | The Expanse | S03E04 | Unknown | GROUP | movie-only CAM ignored |
| `[www.Torrenting.com] - The.Expanse.S03E04.720p.HDTV.x264-DIMENSION` | The Expanse | S03E04 | HDTV-720p | DIMENSION | website prefix stripped |
| `The.Expanse.S03E04.1080p.WEB-DL.x264-NTb-Obfuscated` | The Expanse | S03E04 | WEBDL-1080p | NTb | repost suffix stripped |
| `Band.of.Brothers.Part.01.1080p.BluRay.x264-GROUP` | Band of Brothers | S01E01 | Bluray-1080p | GROUP | mini-series Part 01 |
| `The.Expanse.S03E04.HC.1080p.WEBRip.AV1-GROUP` | The Expanse | S03E04 | WEBRip-1080p | GROUP | hardcoded subs, AV1 |

## Decisions (2026-11-15)

- Size limits reuse Radarr's quality definitions.
- Language: profile language as in Radarr, plus language conditions in custom
  formats. This keeps instance import and the TRaSH guides lossless; a
  separate preferred list would be a second, incompatible mechanism.
- Custom formats and the TRaSH guide browser are in V1.
