import type { components } from "./api/schema.js";

export type Movie = components["schemas"]["Movie"];
export type MovieFile = components["schemas"]["MovieFile"];
export type MovieProfile = components["schemas"]["MovieProfile"];

export type MonitoredFilter = "all" | "monitored" | "unmonitored";
export type SortKey = "title" | "year" | "added";
export type SortDirection = "asc" | "desc";

export interface Filters {
  text: string;
  monitored: MonitoredFilter;
  /** A quality profile id, or null for any. */
  profileId: number | null;
  /** A tag id, or null for any. */
  tagId: number | null;
}

export const noFilters: Filters = {
  text: "",
  monitored: "all",
  profileId: null,
  tagId: null,
};

export const availabilityNames: Record<string, string> = {
  announced: "Announced",
  inCinemas: "In cinemas",
  released: "Released",
};

/** TMDB serves posters in fixed widths; 342 fits a grid card. */
export function posterUrl(
  path: string | null | undefined,
  width = 342,
): string | null {
  return path ? `https://image.tmdb.org/t/p/w${width}${path}` : null;
}

export function matches(movie: Movie, filters: Filters): boolean {
  const text = filters.text.trim().toLowerCase();
  if (
    text &&
    !`${movie.title} ${movie.originalTitle ?? ""} ${movie.year}`
      .toLowerCase()
      .includes(text)
  ) {
    return false;
  }
  if (filters.monitored === "monitored" && !movie.monitored) return false;
  if (filters.monitored === "unmonitored" && movie.monitored) return false;
  if (
    filters.profileId !== null &&
    !movie.profiles.some(
      (profile) => profile.qualityProfileId === filters.profileId,
    )
  ) {
    return false;
  }
  return filters.tagId === null || movie.tags.includes(filters.tagId);
}

const compare: Record<SortKey, (a: Movie, b: Movie) => number> = {
  title: (a, b) => a.sortTitle.localeCompare(b.sortTitle),
  year: (a, b) => a.year - b.year,
  added: (a, b) => a.addedAt.localeCompare(b.addedAt),
};

/** The movies that pass `filters`, in order. Never changes `movies`. */
export function visibleMovies(
  movies: readonly Movie[],
  filters: Filters,
  sort: SortKey,
  direction: SortDirection,
): Movie[] {
  const sign = direction === "asc" ? 1 : -1;
  return movies
    .filter((movie) => matches(movie, filters))
    .sort(
      (a, b) =>
        sign * compare[sort](a, b) || a.sortTitle.localeCompare(b.sortTitle),
    );
}

/** What a movie profile shows on the detail page. */
export function profileState(
  profile: MovieProfile,
  monitored: boolean,
): "Downloaded" | "Missing" | "Not monitored" {
  if (profile.fileId != null) return "Downloaded";
  return monitored ? "Missing" : "Not monitored";
}

/** `1999-03-30` or an ISO time stamp as a date, or a dash. */
export function formatDate(value: string | null | undefined): string {
  return value ? value.slice(0, 10) : "—";
}

export function formatRuntime(minutes: number | null | undefined): string {
  if (!minutes || minutes <= 0) return "—";
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return hours > 0 ? `${hours} h ${rest} min` : `${rest} min`;
}
