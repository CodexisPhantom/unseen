import { describe, expect, it } from "vitest";
import {
  formatDate,
  formatRuntime,
  matches,
  noFilters,
  posterUrl,
  profileState,
  visibleMovies,
  type Movie,
} from "./library.js";

function movie(patch: Partial<Movie>): Movie {
  return {
    id: 1,
    tmdbId: 1,
    title: "Movie",
    sortTitle: "movie",
    originalTitle: null,
    year: 2000,
    monitored: true,
    tags: [],
    profiles: [{ id: 1, qualityProfileId: 10, isPrimary: true, fileId: null }],
    addedAt: "2026-01-01T00:00:00Z",
    ...patch,
  } as Movie;
}

const matrix = movie({
  id: 1,
  title: "The Matrix",
  sortTitle: "matrix",
  year: 1999,
  tags: [5],
});
const alien = movie({
  id: 2,
  title: "Alien",
  sortTitle: "alien",
  year: 1979,
  monitored: false,
  profiles: [{ id: 2, qualityProfileId: 11, isPrimary: true, fileId: 9 }],
  addedAt: "2026-03-01T00:00:00Z",
});
const dune = movie({
  id: 3,
  title: "Dune",
  sortTitle: "dune",
  year: 2021,
  originalTitle: "Duna",
  addedAt: "2026-02-01T00:00:00Z",
});
const all = [matrix, alien, dune];

describe("matches", () => {
  it("lets everything through without filters", () => {
    expect(all.every((m) => matches(m, noFilters))).toBe(true);
  });

  it("filters by text on title, original title and year, ignoring case", () => {
    expect(
      visibleMovies(all, { ...noFilters, text: " MATRIX " }, "title", "asc"),
    ).toEqual([matrix]);
    expect(
      visibleMovies(all, { ...noFilters, text: "duna" }, "title", "asc"),
    ).toEqual([dune]);
    expect(
      visibleMovies(all, { ...noFilters, text: "1979" }, "title", "asc"),
    ).toEqual([alien]);
  });

  it("filters by monitored state, profile and tag", () => {
    const ids = (filters: Partial<typeof noFilters>) =>
      visibleMovies(all, { ...noFilters, ...filters }, "title", "asc").map(
        (m) => m.id,
      );
    expect(ids({ monitored: "monitored" })).toEqual([3, 1]);
    expect(ids({ monitored: "unmonitored" })).toEqual([2]);
    expect(ids({ profileId: 11 })).toEqual([2]);
    expect(ids({ tagId: 5 })).toEqual([1]);
    expect(ids({ tagId: 5, monitored: "unmonitored" })).toEqual([]);
  });
});

describe("visibleMovies", () => {
  const order = (key: "title" | "year" | "added", direction: "asc" | "desc") =>
    visibleMovies(all, noFilters, key, direction).map((m) => m.id);

  it("sorts by title without the leading article, by year and by date added", () => {
    expect(order("title", "asc")).toEqual([2, 3, 1]);
    expect(order("title", "desc")).toEqual([1, 3, 2]);
    expect(order("year", "asc")).toEqual([2, 1, 3]);
    expect(order("added", "desc")).toEqual([2, 3, 1]);
  });

  it("breaks a tie by title and leaves the input alone", () => {
    const twin = movie({
      id: 4,
      title: "Another",
      sortTitle: "another",
      year: 1999,
    });
    const input = [matrix, twin];
    expect(
      visibleMovies(input, noFilters, "year", "asc").map((m) => m.id),
    ).toEqual([4, 1]);
    expect(input).toEqual([matrix, twin]);
  });
});

describe("profileState", () => {
  it("names the state of a profile", () => {
    const profile = {
      id: 1,
      qualityProfileId: 1,
      isPrimary: true,
      fileId: null,
    };
    expect(profileState({ ...profile, fileId: 4 }, true)).toBe("Downloaded");
    expect(profileState(profile, true)).toBe("Missing");
    expect(profileState(profile, false)).toBe("Not monitored");
  });
});

describe("formatting", () => {
  it("builds poster addresses", () => {
    expect(posterUrl("/abc.jpg")).toBe(
      "https://image.tmdb.org/t/p/w342/abc.jpg",
    );
    expect(posterUrl("/abc.jpg", 185)).toBe(
      "https://image.tmdb.org/t/p/w185/abc.jpg",
    );
    expect(posterUrl(null)).toBeNull();
  });

  it("formats dates and runtimes", () => {
    expect(formatDate("2026-03-01T10:00:00Z")).toBe("2026-03-01");
    expect(formatDate(null)).toBe("—");
    expect(formatRuntime(136)).toBe("2 h 16 min");
    expect(formatRuntime(45)).toBe("45 min");
    expect(formatRuntime(null)).toBe("—");
    expect(formatRuntime(0)).toBe("—");
  });
});
