import type { components } from './api/schema.js';

export type CustomFormat = components['schemas']['CustomFormat'];
export type CustomFormatSpecification = components['schemas']['CustomFormatSpecification'];
export type SpecificationField = components['schemas']['SpecificationField'];
export type RadarrCustomFormat = components['schemas']['RadarrCustomFormat'];
export type CustomFormatTestResult = components['schemas']['CustomFormatTestResult'];

export interface OptionItem {
	value: number;
	label: string;
}

export interface SpecificationTypeInfo {
	implementation: string;
	label: string;
	kind: 'regex' | 'select' | 'language' | 'range-float' | 'range-int';
	options?: readonly OptionItem[];
	unit?: string;
}

export const RESOLUTION_OPTIONS: readonly OptionItem[] = [
	{ value: 480, label: '480p' },
	{ value: 576, label: '576p' },
	{ value: 720, label: '720p' },
	{ value: 1080, label: '1080p' },
	{ value: 2160, label: '2160p' }
];

export const SOURCE_OPTIONS: readonly OptionItem[] = [
	{ value: 1, label: 'CAM' },
	{ value: 2, label: 'TELESYNC' },
	{ value: 3, label: 'TELECINE' },
	{ value: 4, label: 'WORKPRINT' },
	{ value: 5, label: 'DVD' },
	{ value: 6, label: 'TV' },
	{ value: 7, label: 'WEB-DL' },
	{ value: 8, label: 'WEBRip' },
	{ value: 9, label: 'BluRay' }
];

export const QUALITY_MODIFIER_OPTIONS: readonly OptionItem[] = [
	{ value: 0, label: 'None' },
	{ value: 1, label: 'Regional' },
	{ value: 2, label: 'Screener' },
	{ value: 3, label: 'Raw-HD' },
	{ value: 4, label: 'BR-Disk' },
	{ value: 5, label: 'Remux' }
];

export const LANGUAGE_OPTIONS: readonly OptionItem[] = [
	{ value: -2, label: 'Original' },
	{ value: -1, label: 'Any' },
	{ value: 1, label: 'English' },
	{ value: 2, label: 'French' },
	{ value: 3, label: 'Spanish' },
	{ value: 4, label: 'German' },
	{ value: 5, label: 'Italian' },
	{ value: 6, label: 'Danish' },
	{ value: 7, label: 'Dutch' },
	{ value: 8, label: 'Japanese' },
	{ value: 10, label: 'Chinese' },
	{ value: 11, label: 'Russian' },
	{ value: 12, label: 'Polish' },
	{ value: 14, label: 'Swedish' },
	{ value: 15, label: 'Norwegian' },
	{ value: 16, label: 'Finnish' },
	{ value: 18, label: 'Portuguese' },
	{ value: 21, label: 'Korean' },
	{ value: 26, label: 'Hindi' }
];

export const INDEXER_FLAG_OPTIONS: readonly OptionItem[] = [
	{ value: 1, label: 'G_Freeleech' },
	{ value: 2, label: 'G_Halfleech' },
	{ value: 4, label: 'G_DoubleUpload' },
	{ value: 8, label: 'PTP_Golden' },
	{ value: 16, label: 'PTP_Approved' },
	{ value: 32, label: 'HDB_Internal' },
	{ value: 64, label: 'AHD_Internal' },
	{ value: 128, label: 'G_Scene' },
	{ value: 256, label: 'G_Freeleech75' },
	{ value: 512, label: 'G_Freeleech25' },
	{ value: 1024, label: 'AHD_UserRelease' },
	{ value: 2048, label: 'Nuked' }
];

export const SPECIFICATION_TYPES: readonly SpecificationTypeInfo[] = [
	{
		implementation: 'ReleaseTitleSpecification',
		label: 'Release Title',
		kind: 'regex'
	},
	{
		implementation: 'ReleaseGroupSpecification',
		label: 'Release Group',
		kind: 'regex'
	},
	{
		implementation: 'EditionSpecification',
		label: 'Edition',
		kind: 'regex'
	},
	{
		implementation: 'LanguageSpecification',
		label: 'Language',
		kind: 'language',
		options: LANGUAGE_OPTIONS
	},
	{
		implementation: 'SourceSpecification',
		label: 'Source',
		kind: 'select',
		options: SOURCE_OPTIONS
	},
	{
		implementation: 'ResolutionSpecification',
		label: 'Resolution',
		kind: 'select',
		options: RESOLUTION_OPTIONS
	},
	{
		implementation: 'QualityModifierSpecification',
		label: 'Quality Modifier',
		kind: 'select',
		options: QUALITY_MODIFIER_OPTIONS
	},
	{
		implementation: 'SizeSpecification',
		label: 'Size',
		kind: 'range-float',
		unit: 'GiB'
	},
	{
		implementation: 'IndexerFlagSpecification',
		label: 'Indexer Flag',
		kind: 'select',
		options: INDEXER_FLAG_OPTIONS
	},
	{
		implementation: 'YearSpecification',
		label: 'Year',
		kind: 'range-int'
	}
];

export function specTypeInfo(implementation: string): SpecificationTypeInfo {
	return (
		SPECIFICATION_TYPES.find((item) => item.implementation === implementation) ?? {
			implementation,
			label: implementation,
			kind: 'regex'
		}
	);
}

export function defaultFieldsFor(implementation: string): SpecificationField[] {
	switch (implementation) {
		case 'ReleaseTitleSpecification':
		case 'ReleaseGroupSpecification':
		case 'EditionSpecification':
			return [{ name: 'value', value: '' as unknown as Record<string, never> }];
		case 'LanguageSpecification':
			return [
				{ name: 'value', value: 1 as unknown as Record<string, never> },
				{ name: 'exceptLanguage', value: false as unknown as Record<string, never> }
			];
		case 'SourceSpecification':
			return [{ name: 'value', value: 9 as unknown as Record<string, never> }];
		case 'ResolutionSpecification':
			return [{ name: 'value', value: 1080 as unknown as Record<string, never> }];
		case 'QualityModifierSpecification':
			return [{ name: 'value', value: 5 as unknown as Record<string, never> }];
		case 'SizeSpecification':
			return [
				{ name: 'min', value: 0 as unknown as Record<string, never> },
				{ name: 'max', value: 50 as unknown as Record<string, never> }
			];
		case 'IndexerFlagSpecification':
			return [{ name: 'value', value: 1 as unknown as Record<string, never> }];
		case 'YearSpecification':
			return [
				{ name: 'min', value: 2000 as unknown as Record<string, never> },
				{ name: 'max', value: 2025 as unknown as Record<string, never> }
			];
		default:
			return [{ name: 'value', value: '' as unknown as Record<string, never> }];
	}
}

export function fieldValue(
	fields: readonly SpecificationField[] | undefined,
	name: string
): unknown {
	return fields?.find((field) => field.name === name)?.value;
}

export function setFieldValue(
	fields: readonly SpecificationField[] | undefined,
	name: string,
	value: unknown
): SpecificationField[] {
	const list = fields ?? [];
	const index = list.findIndex((field) => field.name === name);
	const cast = value as unknown as Record<string, never>;
	if (index >= 0) {
		return list.map((field, i) => (i === index ? { ...field, value: cast } : field));
	}
	return [...list, { name, value: cast }];
}

export interface DraftSpecification {
	key: number;
	name: string;
	implementation: string;
	negate: boolean;
	required: boolean;
	fields: SpecificationField[];
}

export interface CustomFormatDraft {
	name: string;
	includeCustomFormatWhenRenaming: boolean;
	specifications: DraftSpecification[];
}

let nextKey = 1;

export function createDraftSpec(
	implementation = 'ReleaseTitleSpecification'
): DraftSpecification {
	return {
		key: nextKey++,
		name: '',
		implementation,
		negate: false,
		required: false,
		fields: defaultFieldsFor(implementation)
	};
}

export function draftOf(format: CustomFormat | null): CustomFormatDraft {
	if (!format) {
		return {
			name: '',
			includeCustomFormatWhenRenaming: false,
			specifications: [createDraftSpec()]
		};
	}
	return {
		name: format.name,
		includeCustomFormatWhenRenaming: format.includeCustomFormatWhenRenaming,
		specifications: format.specifications.map((spec) => ({
			...structuredClone(spec),
			name: spec.name ?? '',
			negate: spec.negate ?? false,
			required: spec.required ?? false,
			fields: structuredClone(spec.fields ?? []),
			key: nextKey++
		}))
	};
}

export function draftToPayload(draft: CustomFormatDraft): {
	name: string;
	includeCustomFormatWhenRenaming: boolean;
	specifications: CustomFormatSpecification[];
} {
	return {
		name: draft.name.trim(),
		includeCustomFormatWhenRenaming: draft.includeCustomFormatWhenRenaming,
		specifications: draft.specifications.map((spec) => {
			const rest = { ...spec } as Record<string, unknown>;
			delete rest.key;
			return rest as unknown as CustomFormatSpecification;
		})
	};
}

export function draftProblem(draft: CustomFormatDraft): string {
	if (!draft.name.trim()) return 'The name must not be empty.';
	return '';
}

export function optionLabel(options: readonly OptionItem[] | undefined, value: unknown): string {
	const num = Number(value);
	return options?.find((item) => item.value === num)?.label ?? String(value ?? '');
}

export function sourceName(id: number): string {
	return SOURCE_OPTIONS.find((item) => item.value === id)?.label ?? 'Unknown';
}

export function modifierName(id: number): string {
	return QUALITY_MODIFIER_OPTIONS.find((item) => item.value === id)?.label ?? 'None';
}

export function specSummary(spec: CustomFormatSpecification): string {
	const info = specTypeInfo(spec.implementation);
	const prefix = spec.negate ? 'Not ' : '';
	const name = (spec.name ?? '').trim();
	if (name) return `${prefix}${name}`;
	if (info.kind === 'select' || info.kind === 'language') {
		return `${prefix}${info.label}: ${optionLabel(info.options, fieldValue(spec.fields, 'value'))}`;
	}
	if (info.kind === 'range-float' || info.kind === 'range-int') {
		const min = fieldValue(spec.fields, 'min') ?? 0;
		const max = fieldValue(spec.fields, 'max') ?? 0;
		const unit = info.unit ? ` ${info.unit}` : '';
		return `${prefix}${info.label}: ${min}–${max}${unit}`;
	}
	return `${prefix}${info.label}`;
}

export function problemMessage(error: {
	detail?: string | null;
	title?: string | null;
	errors?: Array<{ field: string; message: string }> | null;
}): string {
	if (error.errors && error.errors.length > 0) {
		return error.errors.map((e) => `${e.field}: ${e.message}`).join('; ');
	}
	return error.detail ?? error.title ?? 'Request failed.';
}
