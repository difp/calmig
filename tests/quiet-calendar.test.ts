/* eslint-disable @typescript-eslint/no-unsafe-call */
import test from 'ava';
import {
	syncCalendar,
	toPhoto,
	type CalendarSource,
	type CalendarState,
} from '../source/quiet/calendar.js';

const now = Date.parse('2026-10-31T12:00:00Z');
const day = 86_400_000;
const account = {pk: '7', username: 'friend'};
const image = {
	media_type: 1,
	image_versions2: {
		candidates: [{url: 'https://scontent.cdninstagram.com/a.jpg'}],
	},
};
function post(id: string, age: number) {
	return {...image, pk: id, code: id, taken_at: (now - age * day) / 1000};
}

function page<T>(items: T[]) {
	return {
		async items() {
			return items;
		},
		isMoreAvailable: () => false,
	};
}

test('photos are limited to the week and exclude video and untrusted URLs', t => {
	const cutoff = now - 7 * day;
	t.truthy(toPhoto(post('ok', 1), account, cutoff, now));
	t.is(
		toPhoto({...post('video', 1), media_type: 2}, account, cutoff, now),
		undefined,
	);
	t.is(toPhoto(post('old', 9), account, cutoff, now), undefined);
	t.is(
		toPhoto(
			{
				...post('mixed', 1),
				media_type: 8,
				carousel_media: [image, {media_type: 2}],
			},
			account,
			cutoff,
			now,
		),
		undefined,
	);
	t.is(
		toPhoto(
			{
				...post('foreign', 1),
				image_versions2: {candidates: [{url: 'https://example.com/photo.jpg'}]},
			},
			account,
			cutoff,
			now,
		),
		undefined,
	);
});
test('keeps the author caption as plain text', t => {
	const result = toPhoto(
		{...post('captioned', 1), caption: {text: '<b>Привет</b>\nТекст'}},
		account,
		now - 7 * day,
		now,
	);
	t.is(result?.caption, '<b>Привет</b>\nТекст');
});
test('sync uses followed accounts and sorts photos chronologically', async t => {
	const queried: string[] = [];
	const source: CalendarSource = {
		following: () => page([account, {pk: '8', username: 'second'}]),
		posts(id) {
			queried.push(id);
			return page(id === '7' ? [post('older', 2)] : [post('newer', 1)]);
		},
	};
	let final: CalendarState | undefined;
	await syncCalendar(
		source,
		state => {
			final = state;
		},
		new AbortController().signal,
		{now, delayMs: 0},
	);
	t.deepEqual(queried, ['7', '8']);
	t.is(final?.status, 'complete');
	t.deepEqual(
		final?.photos.map(item => item.id),
		['newer', 'older'],
	);
});
test('request failure is not presented as a complete selection', async t => {
	const source: CalendarSource = {
		following: () => page([account]),
		posts: () => ({
			async items() {
				throw new Error('limited');
			},
			isMoreAvailable: () => false,
		}),
	};
	let final: CalendarState | undefined;
	await syncCalendar(
		source,
		state => {
			final = state;
		},
		new AbortController().signal,
		{now, delayMs: 0},
	);
	t.is(final?.status, 'error');
	t.true(final?.incomplete);
});

test('unsafe numeric account IDs are not silently rounded', async t => {
	const queried: string[] = [];
	const source: CalendarSource = {
		following: () => page([{pk: 9_007_199_254_740_992, username: 'friend'}]),
		posts(id) {
			queried.push(id);
			return page([]);
		},
	};
	let final: CalendarState | undefined;
	await syncCalendar(
		source,
		state => {
			final = state;
		},
		new AbortController().signal,
		{now, delayMs: 0},
	);
	t.deepEqual(queried, []);
	t.true(final?.incomplete);
});

test('slow profile requests overlap instead of blocking every following account', async t => {
	let active = 0;
	let peak = 0;
	const source: CalendarSource = {
		following: () =>
			page([
				{pk: '7', username: 'first'},
				{pk: '8', username: 'second'},
				{pk: '9', username: 'third'},
			]),
		posts: () => ({
			async items() {
				active++;
				peak = Math.max(peak, active);
				await new Promise<void>(resolve => {
					setTimeout(resolve, 80);
				});
				active--;
				return [];
			},
			isMoreAvailable: () => false,
		}),
	};
	await syncCalendar(source, () => {}, new AbortController().signal, {
		now,
		delayMs: 0,
	});
	t.true(peak > 1, 'independent profile requests should overlap');
});

test('an error stops queued requests and reports an incomplete selection', async t => {
	let started = 0;
	const source: CalendarSource = {
		following: () =>
			page([
				{pk: '7', username: 'first'},
				{pk: '8', username: 'second'},
				{pk: '9', username: 'third'},
			]),
		posts: () => ({
			async items() {
				started++;
				throw new Error('rate limit');
			},
			isMoreAvailable: () => false,
		}),
	};
	let final: CalendarState | undefined;
	await syncCalendar(
		source,
		state => {
			final = state;
		},
		new AbortController().signal,
		{now, delayMs: 100},
	);
	t.is(started, 1);
	t.is(final?.status, 'error');
	t.true(final?.incomplete);
});
