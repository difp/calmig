/* eslint-disable @typescript-eslint/no-unsafe-call */
import test from 'ava';
import {authorDetails, mediaIdFromCode} from '../source/quiet/photo-details.js';

test('decodes a shortcode without rounding a large media ID', t => {
	t.is(mediaIdFromCode('BA'), '64');
	t.is(mediaIdFromCode('__________'), '1152921504606846975');
	t.throws(() => mediaIdFromCode('../bad'));
});

test('returns comments from the author and other users', t => {
	const details = authorDetails({
		caption: {text: 'Подпись'},
		comments: [
			{user: {username: 'Author'}, text: 'Комментарий автора'},
			{user: {username: 'other'}, text: 'Чужой комментарий'},
		],
	});
	t.is(details.caption, 'Подпись');
	t.deepEqual(details.comments, [
		{username: 'Author', text: 'Комментарий автора'},
		{username: 'other', text: 'Чужой комментарий'},
	]);
});
