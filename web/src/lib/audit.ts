/** `role.updated` as a label: `Role updated`. */
export function actionLabel(action: string): string {
	const text = action.replace(/[._]/g, ' ');
	return text.charAt(0).toUpperCase() + text.slice(1);
}

/** The target of a row, for a table cell. */
export function targetLabel(kind: string | null | undefined, id: string | null | undefined): string {
	if (!kind) return '';
	return id ? `${kind} ${id}` : kind;
}

/** The changed fields of a row as readable JSON, or an empty text. */
export function dataText(data: unknown): string {
	if (data === null || data === undefined) return '';
	return JSON.stringify(data, null, 2);
}

/** Pages of `total` rows, at least 1. */
export function pageCount(total: number, pageSize: number): number {
	return Math.max(1, Math.ceil(total / pageSize));
}

/** The start of a day from a date input (`2026-11-24`), in local time, as an ISO time. */
export function startOfDay(day: string, offset = 0): string | undefined {
	if (!day) return undefined;
	const start = new Date(`${day}T00:00:00`);
	start.setDate(start.getDate() + offset);
	return start.toISOString();
}
