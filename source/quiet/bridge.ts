/** Local JSON-lines bridge for the quiet desktop. Never prints API data to logs. */
import readline from 'node:readline';
import process from 'node:process';
import type {MediaCommentsFeed, UserFeed} from 'instagram-private-api';
import {InstagramClient} from '../client.js';
import {ConfigManager} from '../config.js';
import {
	syncCalendar,
	toPhoto,
	type CalendarSource,
	type CalendarState,
	type PhotoPost,
} from './calendar.js';
import {clearCache, loadCache, saveCache} from './cache.js';
import {enforceTls} from './secure-api.js';
import {loginFailure} from './login-errors.js';
import {attachBrowserCookies, type BrowserCookie} from './browser-session.js';
import {authorDetails, mediaIdFromCode} from './photo-details.js';
import {followingPages} from './following-page.js';

// Library output must never corrupt the protocol or expose credentials.
console.log = () => {};
console.warn = () => {};
console.error = () => {};
let client: InstagramClient | undefined;
let account = '';
let pending: InstagramClient | undefined;
let browserSession = false;
let job: AbortController | undefined;
let jobDone: Promise<void> | undefined;
let generation = 0;
let calendarRevision = 0;
let cacheWrite: Promise<void> | undefined;
let cacheTimer: ReturnType<typeof setTimeout> | undefined;
let profile:
	| {
			username: string;
			feed: UserFeed;
			posts: Map<string, PhotoPost>;
			loading: boolean;
	  }
	| undefined;
let profileGeneration = 0;
let commentFeed:
	| {code: string; feed: MediaCommentsFeed; loading: boolean}
	| undefined;
let calendar: CalendarState = {
	status: 'complete',
	accounts: 0,
	photos: [],
	incomplete: false,
};
const allowedThreads = new Set<string>();
const sendingCommentPosts = new Set<string>();
const messageCursors = new Map<string, string>();

type Request = {
	id: number;
	action: string;
	username?: string;
	password?: string;
	code?: string;
	thread?: string;
	text?: string;
	cursor?: string;
	more?: boolean;
	revision?: number;
	cookies?: BrowserCookie[];
};
function reset(): void {
	generation++;
	profileGeneration++;
	profile = undefined;
	commentFeed = undefined;
	clearTimeout(cacheTimer);
	cacheTimer = undefined;
	job?.abort();
	job = undefined;
	calendar = {status: 'complete', accounts: 0, photos: [], incomplete: false};
	calendarRevision++;
	allowedThreads.clear();
	messageCursors.clear();
}

function requireClient(): InstagramClient {
	if (!client) throw new Error('not_connected');
	return client;
}

function safeUsername(username: string): boolean {
	return (
		typeof username === 'string' &&
		/^[\w.]{1,30}$/.test(username) &&
		username !== '.' &&
		username !== '..'
	);
}

async function restoreCalendar(username: string): Promise<void> {
	const photos = await loadCache(
		ConfigManager.getInstance().get('advanced.cacheDir'),
		username,
	);
	calendar = {
		status: 'complete',
		accounts: 0,
		photos,
		incomplete: true,
		cached: photos.length > 0,
	};
	calendarRevision++;
}

async function handle(request: Request): Promise<unknown> {
	switch (request.action) {
		case 'status': {
			return {connected: Boolean(client), username: account};
		}

		case 'login': {
			const username = request.username?.trim() ?? '';
			if (
				!safeUsername(username) ||
				!request.password ||
				request.password.length > 1024 ||
				pending !== undefined ||
				client !== undefined
			)
				throw new Error('invalid_login');
			await ConfigManager.getInstance().initialize();
			pending = new InstagramClient(username);
			enforceTls(pending.getInstagramClient());
			const result = await pending.login(username, request.password, {
				initializeRealtime: false,
				initializeFeed: false,
			});
			request.password = undefined;
			if (result.success) {
				client = pending;
				pending = undefined;
				account = username;
				await restoreCalendar(account);
				return {connected: true, username};
			}

			if (result.verificationMessage) return {verification: true};
			pending = undefined;
			return {connected: false, reason: loginFailure(result)};
		}

		case 'code': {
			if (
				!pending ||
				!request.code ||
				!/^[A-Za-z\d -]{4,20}$/.test(request.code)
			)
				throw new Error('invalid_code');
			const result = await pending.submitLoginCode(request.code, {
				initializeFeed: false,
			});
			request.code = undefined;
			if (result.success) {
				client = pending;
				account = result.username ?? pending.getUsername() ?? '';
				pending = undefined;
				await restoreCalendar(account);
				return {connected: true, username: account};
			}

			if (result.verificationMessage) return {verification: true};
			pending = undefined;
			return {connected: false, reason: loginFailure(result)};
		}

		case 'cancel-login': {
			if (pending) await pending.shutdown();
			pending = undefined;
			return {cancelled: true};
		}

		case 'browser-login': {
			if (
				client !== undefined ||
				pending !== undefined ||
				!Array.isArray(request.cookies)
			)
				throw new Error('invalid_login');
			await ConfigManager.getInstance().initialize();
			const candidate = new InstagramClient('browser-session');
			pending = candidate;
			try {
				const ig = candidate.getInstagramClient();
				const userId = attachBrowserCookies(ig, request.cookies);
				request.cookies = undefined;
				const user = await ig.user.info(userId);
				if (!safeUsername(user.username))
					throw new Error('invalid_browser_session');
				const settings = await ig.state.serialize();
				const connected = new InstagramClient(user.username);
				const restored = connected.getInstagramClient();
				await restored.state.deserialize(settings);
				enforceTls(restored);
				client = connected;
				account = user.username;
				browserSession = true;
				await restoreCalendar(account);
				return {connected: true, username: account};
			} finally {
				request.cookies = undefined;
				pending = undefined;
			}
		}

		case 'restore': {
			const username = request.username?.trim() ?? '';
			if (
				!safeUsername(username) ||
				client !== undefined ||
				pending !== undefined
			)
				throw new Error('invalid_login');
			await ConfigManager.getInstance().initialize();
			const candidate = new InstagramClient(username);
			enforceTls(candidate.getInstagramClient());
			const result = await candidate.loginBySession({
				initializeRealtime: false,
				initializeFeed: false,
			});
			if (!result.success) throw new Error('session_unavailable');
			client = candidate;
			account = username;
			await restoreCalendar(account);
			return {connected: true, username};
		}

		case 'disconnect': {
			const oldAccount = account;
			const wasBrowserSession = browserSession;
			browserSession = false;
			const previousJob = jobDone;
			reset();
			const previous = client;
			client = undefined;
			pending = undefined;
			account = '';
			if (previousJob) await previousJob;
			if (cacheWrite) await cacheWrite;
			if (oldAccount)
				await clearCache(
					ConfigManager.getInstance().get('advanced.cacheDir'),
					oldAccount,
				);
			if (previous) {
				if (!wasBrowserSession) await previous.logout();
				await previous.shutdown();
			}

			return {connected: false};
		}

		case 'calendar-start': {
			const active = requireClient();
			if (job) throw new Error('sync_in_progress');
			const ig = active.getInstagramClient();
			const source: CalendarSource = {
				following: () =>
					followingPages(async (cursor, count) => {
						const id = ig.state.cookieUserId;
						if (!/^\d{1,30}$/.test(id)) throw new Error('invalid_account');
						const {body} = await ig.request.send<{
							users?: unknown;
							next_max_id?: unknown;
						}>({
							url: `/api/v1/friendships/${id}/following/`,
							qs: {
								count,
								max_id: cursor,
								order: 'default',
								query: '',
								enable_groups: false,
								includes_hashtags: false,
							},
						});
						return body;
					}),
				posts: id => ig.feed.user(id),
			};
			const controller = new AbortController();
			const previousPhotos = calendar.photos;
			const activeAccount = account;
			job = controller;
			const current = ++generation;
			let savedPhotos = '';
			const queueCache = async () => {
				const photos = [...calendar.photos];
				cacheWrite = (cacheWrite ?? Promise.resolve())
					.then(async () => {
						if (generation !== current) return;
						await saveCache(
							ConfigManager.getInstance().get('advanced.cacheDir'),
							activeAccount,
							photos,
						);
					})
					.catch(() => {});
				return cacheWrite;
			};

			calendar = {
				status: 'loading',
				accounts: 0,
				photos: previousPhotos,
				incomplete: false,
				cached: previousPhotos.length > 0,
			};
			calendarRevision++;
			jobDone = syncCalendar(
				source,
				state => {
					if (generation !== current) return;
					if (
						state.status === 'loading' ||
						state.status === 'error' ||
						state.status === 'cancelled' ||
						state.incomplete
					) {
						const merged = new Map(previousPhotos.map(post => [post.id, post]));
						for (const post of state.photos) merged.set(post.id, post);
						calendar = {
							...state,
							photos: [...merged.values()].sort(
								(a, b) => Date.parse(b.publishedAt) - Date.parse(a.publishedAt),
							),
							cached: previousPhotos.length > 0,
						};
					} else calendar = state;
					calendarRevision++;
					const photoStamp = JSON.stringify(
						calendar.photos.map(post => [
							post.id,
							post.thumbnail,
							post.caption,
						]),
					);
					if (
						state.status === 'loading' &&
						photoStamp !== savedPhotos &&
						calendar.photos.length > 0
					) {
						savedPhotos = photoStamp;
						cacheTimer ??= setTimeout(() => {
							cacheTimer = undefined;
							if (generation === current) void queueCache();
						}, 1500);
					}
				},
				controller.signal,
			)
				.then(async () => {
					if (generation !== current) return;
					clearTimeout(cacheTimer);
					cacheTimer = undefined;
					await queueCache();
				})
				.finally(() => {
					if (generation === current) {
						job = undefined;
						jobDone = undefined;
					}
				});
			return {started: true};
		}

		case 'calendar-status': {
			requireClient();
			if (request.revision === calendarRevision)
				return {unchanged: true, revision: calendarRevision};
			return {...calendar, revision: calendarRevision};
		}

		case 'photo-details': {
			const active = requireClient();
			const photo =
				calendar.photos.find(post => post.id === request.code) ??
				profile?.posts.get(request.code ?? '');
			if (!photo) throw new Error('invalid_post');
			const feed = active
				.getInstagramClient()
				.feed.mediaComments(mediaIdFromCode(photo.id));
			const current = {code: photo.id, feed, loading: true};
			commentFeed = current;
			try {
				const result = await feed.request();
				if (commentFeed !== current) throw new Error('photo_changed');
				return {...authorDetails(result), more: feed.isMoreAvailable()};
			} finally {
				current.loading = false;
			}
		}

		case 'photo-comments-more': {
			requireClient();
			const current = commentFeed;
			if (
				!current ||
				current.code !== request.code ||
				current.loading ||
				!current.feed.isMoreAvailable()
			)
				throw new Error('invalid_post');
			current.loading = true;
			try {
				const result = await current.feed.request();
				if (commentFeed !== current) throw new Error('photo_changed');
				return {...authorDetails(result), more: current.feed.isMoreAvailable()};
			} finally {
				current.loading = false;
			}
		}

		case 'photo-comment-send': {
			const active = requireClient();
			const photo =
				calendar.photos.find(post => post.id === request.code) ??
				profile?.posts.get(request.code ?? '');
			const text = request.text?.trim() ?? '';
			if (
				!photo ||
				!text ||
				text.length > 1800 ||
				sendingCommentPosts.has(photo.id)
			)
				throw new Error('invalid_comment');
			const sendingAccount = account;
			sendingCommentPosts.add(photo.id);
			try {
				const result = await active
					.getInstagramClient()
					.media.comment({mediaId: mediaIdFromCode(photo.id), text});
				if (!result?.pk) throw new Error('comment_unconfirmed');
				return {sent: true, username: sendingAccount};
			} finally {
				sendingCommentPosts.delete(photo.id);
			}
		}

		case 'profile-open': {
			const active = requireClient();
			const username = request.username ?? '';
			if (
				!safeUsername(username) ||
				!calendar.photos.some(
					post => post.username.toLowerCase() === username.toLowerCase(),
				)
			)
				throw new Error('invalid_profile');
			const current = ++profileGeneration;
			profile = undefined;
			const ig = active.getInstagramClient();
			const user = await ig.user.usernameinfo(username);
			const extendedUser = user as typeof user & {pk_id?: string; id?: string};
			const rawId = extendedUser.pk_id ?? extendedUser.id ?? user.pk;
			const id =
				typeof rawId === 'number' && !Number.isSafeInteger(rawId)
					? ''
					: String(rawId);
			if (
				!/^\d{1,30}$/.test(id) ||
				!safeUsername(user.username) ||
				user.username.toLowerCase() !== username.toLowerCase() ||
				current !== profileGeneration
			)
				throw new Error('invalid_profile');
			const feed = ig.feed.user(id);
			const items = await feed.items();
			if (current !== profileGeneration) throw new Error('profile_changed');
			const photos = items
				.map(item =>
					toPhoto(item, {pk: id, username: user.username}, 0, Date.now()),
				)
				.filter((post): post is PhotoPost => post !== undefined);
			profile = {
				username: user.username,
				feed,
				posts: new Map(photos.map(post => [post.id, post])),
				loading: false,
			};
			return {
				username: user.username,
				name: user.full_name?.slice(0, 150) ?? '',
				bio: user.biography?.slice(0, 2000) ?? '',
				photos,
				more: feed.isMoreAvailable(),
			};
		}

		case 'profile-more': {
			requireClient();
			const current = profile;
			if (!current || current.loading || !current.feed.isMoreAvailable())
				throw new Error('invalid_profile');
			current.loading = true;
			try {
				const items = await current.feed.items();
				if (profile !== current) throw new Error('profile_changed');
				const photos = items
					.map(item =>
						toPhoto(item, {pk: '0', username: current.username}, 0, Date.now()),
					)
					.filter((post): post is PhotoPost => post !== undefined);
				for (const post of photos) current.posts.set(post.id, post);
				return {photos, more: current.feed.isMoreAvailable()};
			} finally {
				current.loading = false;
			}
		}

		case 'calendar-stop': {
			requireClient();
			job?.abort();
			return {stopping: true};
		}

		case 'calendar-clear': {
			requireClient();
			const currentAccount = account;
			const previousJob = jobDone;
			reset();
			if (previousJob) await previousJob;
			if (cacheWrite) await cacheWrite;
			await clearCache(
				ConfigManager.getInstance().get('advanced.cacheDir'),
				currentAccount,
			);
			return {cleared: true};
		}

		case 'threads': {
			const more = request.more === true;
			const result = await requireClient().getThreads(more);
			if (!more) {
				allowedThreads.clear();
				messageCursors.clear();
			}

			for (const thread of result.threads) allowedThreads.add(thread.id);
			return {
				hasMore: result.hasMore,
				threads: result.threads.map(({id, title, unread}) => ({
					id,
					title,
					unread,
				})),
			};
		}

		case 'messages': {
			if (!request.thread || !allowedThreads.has(request.thread))
				throw new Error('invalid_thread');
			if (
				request.cursor &&
				(request.cursor.length > 2048 ||
					messageCursors.get(request.thread) !== request.cursor)
			)
				throw new Error('invalid_cursor');
			const result = await requireClient().getMessages(
				request.thread,
				request.cursor,
			);
			if (result.cursor) messageCursors.set(request.thread, result.cursor);
			else messageCursors.delete(request.thread);
			return {
				cursor: result.cursor ?? null,
				messages: result.messages.map(message => ({
					id: message.id,
					mine: message.isOutgoing,
					attachment: !['text', 'placeholder', 'link'].includes(
						message.itemType,
					),
					text:
						message.itemType === 'text' || message.itemType === 'placeholder'
							? message.text
							: message.itemType === 'link'
								? message.link.text
								: 'Вложение',
					timestamp: message.timestamp,
				})),
			};
		}

		case 'send': {
			if (
				!request.thread ||
				!allowedThreads.has(request.thread) ||
				!request.text?.trim() ||
				request.text.length > 1000
			)
				throw new Error('invalid_message');
			await requireClient().sendMessage(request.thread, request.text.trim());
			return {sent: true};
		}

		default: {
			throw new Error('unknown_action');
		}
	}
}

const input = readline.createInterface({
	input: process.stdin,
	crlfDelay: Infinity,
});
input.on('line', line => {
	if (line.length > 8192) return;
	let request: Request;
	try {
		request = JSON.parse(line) as Request;
	} catch {
		return;
	}

	if (
		!request ||
		typeof request !== 'object' ||
		!Number.isSafeInteger(request.id) ||
		typeof request.action !== 'string'
	)
		return;
	void handle(request)
		.then(data =>
			process.stdout.write(
				`${JSON.stringify({id: request.id, ok: true, data})}\n`,
			),
		)
		.catch(() =>
			process.stdout.write(`${JSON.stringify({id: request.id, ok: false})}\n`),
		);
});
