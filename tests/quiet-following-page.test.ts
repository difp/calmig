/* eslint-disable @typescript-eslint/no-unsafe-call */
import test from 'ava';
import {followingPages} from '../source/quiet/following-page.js';

test('requests larger pages and follows Instagram cursors even if pages are shorter', async t => {
	const calls: Array<{cursor: string | undefined; count: number}> = [];
	const pages = followingPages(async (cursor, count) => {
		calls.push({cursor, count});
		return cursor
			? {users: [{pk: '2', username: 'second'}], next_max_id: null}
			: {users: [{pk: '1', username: 'first'}], next_max_id: 'next'};
	});
	const first = await pages.items();
	t.is(first[0]?.username, 'first');
	t.true(pages.isMoreAvailable());
	const second = await pages.items();
	t.is(second[0]?.username, 'second');
	t.false(pages.isMoreAvailable());
	t.deepEqual(calls, [
		{cursor: undefined, count: 200},
		{cursor: 'next', count: 200},
	]);
});

test('rejects repeated cursors rather than silently looping or claiming completeness', async t => {
	const pages = followingPages(async () => ({users: [], next_max_id: 'same'}));
	await pages.items();
	await t.throwsAsync(pages.items());
});
