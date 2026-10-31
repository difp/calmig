/** Account-scoped local metadata cache. It contains no login credentials. */
import {createHash, randomBytes} from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';
import type {PhotoPost} from './calendar.js';

const limit = 250;

function filename(directory: string, username: string): string {
	const digest = createHash('sha256')
		.update(username.toLowerCase())
		.digest('hex');
	return path.join(directory, `tikho-${digest}.json`);
}

function trustedImage(value: unknown): value is string {
	if (typeof value !== 'string') return false;
	try {
		const url = new URL(value);
		return (
			url.protocol === 'https:' &&
			!url.username &&
			!url.password &&
			(!url.port || url.port === '443') &&
			['cdninstagram.com', 'fbcdn.net', 'instagram.com'].some(
				host => url.hostname === host || url.hostname.endsWith(`.${host}`),
			)
		);
	} catch {
		return false;
	}
}

function validPost(value: unknown, now: number): value is PhotoPost {
	if (!value || typeof value !== 'object') return false;
	const post = value as Partial<PhotoPost>;
	const cutoff = new Date(now);
	cutoff.setHours(0, 0, 0, 0);
	cutoff.setDate(cutoff.getDate() - 6);
	const time = Date.parse(post.publishedAt ?? '');
	return (
		typeof post.id === 'string' &&
		post.id.length < 100 &&
		(post.caption === undefined ||
			(typeof post.caption === 'string' && post.caption.length <= 4000)) &&
		typeof post.username === 'string' &&
		/^[\w.]{1,30}$/.test(post.username) &&
		typeof post.url === 'string' &&
		/^https:\/\/www\.instagram\.com\/p\/[\w-]+\/$/.test(post.url) &&
		Number.isFinite(time) &&
		time >= cutoff.getTime() &&
		time <= now &&
		trustedImage(post.thumbnail) &&
		Array.isArray(post.images) &&
		post.images.length > 0 &&
		post.images.length <= 20 &&
		post.images.every(image => trustedImage(image))
	);
}

export async function loadCache(
	directory: string,
	username: string,
	now = Date.now(),
): Promise<PhotoPost[]> {
	try {
		const file = filename(directory, username);
		const info = await fs.stat(file);
		if (info.size > 2_000_000) return [];
		const data: unknown = JSON.parse(await fs.readFile(file, 'utf8'));
		if (!Array.isArray(data)) return [];
		return data.filter(item => validPost(item, now)).slice(0, limit);
	} catch {
		return [];
	}
}

export async function saveCache(
	directory: string,
	username: string,
	photos: PhotoPost[],
	now = Date.now(),
): Promise<void> {
	const safe = photos.filter(item => validPost(item, now)).slice(0, limit);
	await fs.mkdir(directory, {recursive: true, mode: 0o700});
	const destination = filename(directory, username);
	const temporary = path.join(
		directory,
		`.tikho-${randomBytes(12).toString('hex')}.tmp`,
	);
	try {
		await fs.writeFile(temporary, JSON.stringify(safe), {
			mode: 0o600,
			flag: 'wx',
		});
		await fs.rename(temporary, destination);
	} finally {
		await fs.rm(temporary, {force: true});
	}
}

export async function clearCache(
	directory: string,
	username: string,
): Promise<void> {
	await fs.rm(filename(directory, username), {force: true});
}
