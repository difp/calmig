/* eslint-disable @typescript-eslint/no-unsafe-call */
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'ava';
import {clearCache, loadCache, saveCache} from '../source/quiet/cache.js';
import type {PhotoPost} from '../source/quiet/calendar.js';

test('cache is account scoped, owner only, bounded to recent trusted photos and removable', async t => {
	const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'tikho-cache-'));
	const now = Date.now();
	const post: PhotoPost = {
		id: '1',
		username: 'friend',
		url: 'https://www.instagram.com/p/ABC/',
		thumbnail: 'https://scontent.cdninstagram.com/t.jpg',
		images: ['https://scontent.cdninstagram.com/a.jpg'],
		publishedAt: new Date(now).toISOString(),
		caption: 'Подпись автора',
	};
	try {
		await saveCache(
			directory,
			'first',
			[
				post,
				{
					...post,
					id: 'old',
					publishedAt: new Date(now - 9 * 86_400_000).toISOString(),
				},
				{...post, id: 'unsafe', thumbnail: 'https://example.com/image.jpg'},
			],
			now,
		);
		const cached = await loadCache(directory, 'first', now);
		t.is(cached[0]?.caption, 'Подпись автора');
		t.deepEqual(
			cached.map(item => item.id),
			['1'],
		);
		t.deepEqual(await loadCache(directory, 'second', now), []);
		const files = await fs.readdir(directory);
		t.is(files.length, 1);
		const file = files[0];
		if (!file) throw new Error('missing cache');
		const info = await fs.stat(path.join(directory, file));
		t.is(info.mode & 0o777, 0o600);
		await clearCache(directory, 'first');
		t.deepEqual(await loadCache(directory, 'first', now), []);
	} finally {
		await fs.rm(directory, {recursive: true, force: true});
	}
});
