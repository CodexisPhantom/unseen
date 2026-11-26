import { describe, expect, it } from 'vitest';
import {
	createDraftSpec,
	defaultFieldsFor,
	draftOf,
	draftProblem,
	draftToPayload,
	fieldValue,
	problemMessage,
	setFieldValue,
	specSummary,
	specTypeInfo
} from './custom-formats.js';

describe('custom-formats helpers', () => {
	it('creates a new draft with one default specification', () => {
		const draft = draftOf(null);
		expect(draft.name).toBe('');
		expect(draft.includeCustomFormatWhenRenaming).toBe(false);
		expect(draft.specifications).toHaveLength(1);
		expect(draft.specifications[0].implementation).toBe('ReleaseTitleSpecification');
		expect(draftProblem(draft)).toBe('The name must not be empty.');
	});

	it('strips local keys when serializing draft to API payload', () => {
		const draft = draftOf(null);
		draft.name = '  x265  ';
		const payload = draftToPayload(draft);
		expect(payload.name).toBe('x265');
		expect('key' in payload.specifications[0]).toBe(false);
		expect(draftProblem(draft)).toBe('');
	});

	it('gets and updates specification field values', () => {
		const fields = defaultFieldsFor('SizeSpecification');
		expect(fieldValue(fields, 'min')).toBe(0);
		expect(fieldValue(fields, 'max')).toBe(50);

		const updated = setFieldValue(fields, 'min', 10);
		expect(fieldValue(updated, 'min')).toBe(10);
		expect(fieldValue(updated, 'max')).toBe(50);

		const withExtra = setFieldValue(updated, 'custom', true);
		expect(fieldValue(withExtra, 'custom')).toBe(true);
	});

	it('formats summaries for different condition types', () => {
		const res = createDraftSpec('ResolutionSpecification');
		res.negate = true;
		expect(specSummary(res)).toBe('Not Resolution: 1080p');

		const size = createDraftSpec('SizeSpecification');
		expect(specSummary(size)).toBe('Size: 0–50 GiB');

		const named = createDraftSpec('ReleaseTitleSpecification');
		named.name = 'HEVC';
		expect(specSummary(named)).toBe('HEVC');

		expect(specTypeInfo('UnknownSpecification').label).toBe('UnknownSpecification');
	});

	it('formats RFC 9457 validation errors with field and position details', () => {
		expect(
			problemMessage({
				title: 'Validation failed',
				errors: [
					{
						field: 'specifications[0].fields.value',
						message: 'invalid regex at position 3: Unmatched close paren'
					}
				]
			})
		).toBe(
			'specifications[0].fields.value: invalid regex at position 3: Unmatched close paren'
		);
	});
});
