/** Chronological photos from followed accounts. No home timeline is used. */
export type PhotoPost = {
	id: string;
	username: string;
	url: string;
	images: string[];
	thumbnail: string;
	publishedAt: string;
	caption?: string;
};

type PhotoPage<T> = {
	items(): Promise<T[]>;
	isMoreAvailable(): boolean;
};

type Account = {pk: string | number; pk_id?: string; username: string};
type ImageCandidate = {url?: string; width?: number};
type Resource = {
	media_type?: number;
	image_versions2?: {candidates?: ImageCandidate[]};
	video_versions?: unknown;
};
type Media = Resource & {
	pk?: string | number;
	code?: string;
	taken_at?: number;
	carousel_media?: Resource[];
	is_paid_partnership?: boolean;
	// Instagram returns null when a post has no caption.
	// eslint-disable-next-line @typescript-eslint/no-restricted-types
	caption?: {text?: string} | null;
};

export type CalendarSource = {
	following(): PhotoPage<Account>;
	posts(id: string): PhotoPage<Media>;
};

export type CalendarState = {
	status: 'loading' | 'complete' | 'cancelled' | 'error';
	accounts: number;
	photos: PhotoPost[];
	incomplete: boolean;
	cached?: boolean;
};

function photoUrl(resource: Resource, thumbnail = false): string | undefined {
	const candidates = resource.image_versions2?.candidates?.filter(candidate => {
		try {
			const url = new URL(candidate.url ?? '');
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
	});
	if (!candidates?.length) return undefined;
	const sized = candidates.filter(
		candidate => typeof candidate.width === 'number',
	);
	if (sized.length === 0) return candidates[0]?.url;
	sized.sort((a, b) => (a.width ?? 0) - (b.width ?? 0));
	return (
		thumbnail
			? (sized.find(candidate => (candidate.width ?? 0) >= 320) ?? sized.at(-1))
			: sized.at(-1)
	)?.url;
}

export function toPhoto(
	media: Media,
	account: Account,
	cutoff: number,
	now: number,
): PhotoPost | undefined {
	const timestamp = (media.taken_at ?? 0) * 1000;
	if (
		!Number.isFinite(timestamp) ||
		timestamp < cutoff ||
		timestamp > now ||
		media.is_paid_partnership === true ||
		!media.code ||
		!/^[\w-]+$/.test(media.code)
	)
		return undefined;
	const resources =
		media.media_type === 1
			? [media]
			: media.media_type === 8
				? media.carousel_media
				: undefined;
	if (
		!resources?.length ||
		resources.some(item => item.media_type !== 1 || item.video_versions)
	)
		return undefined;
	const images = resources.map(resource => photoUrl(resource));
	if (images.some(url => !url)) return undefined;
	const first = resources[0];
	if (!first) return undefined;
	const thumbnail = photoUrl(first, true);
	if (!thumbnail) return undefined;
	return {
		id: media.code,
		username: account.username,
		url: `https://www.instagram.com/p/${media.code}/`,
		images: images as string[],
		thumbnail,
		publishedAt: new Date(timestamp).toISOString(),
		caption:
			typeof media.caption?.text === 'string'
				? media.caption.text.slice(0, 4000)
				: '',
	};
}

/** Fetch incrementally. A failed request stops the job; it is never presented as complete. */
export async function syncCalendar(
	source: CalendarSource,
	onUpdate: (state: CalendarState) => void,
	signal: AbortSignal,
	options: {
		now?: number;
		delayMs?: number;
		concurrency?: number;
		maxPagesPerAccount?: number;
	} = {},
): Promise<void> {
	const now = options.now ?? Date.now();
	const cutoff = new Date(now);
	cutoff.setHours(0, 0, 0, 0);
	cutoff.setDate(cutoff.getDate() - 6);
	const state: CalendarState = {
		status: 'loading',
		accounts: 0,
		photos: [],
		incomplete: false,
	};
	const posts = new Map<string, PhotoPost>();
	const users = new Set<string>();
	const publish = () => {
		onUpdate({
			...state,
			photos: [...posts.values()].sort(
				(a, b) => Date.parse(b.publishedAt) - Date.parse(a.publishedAt),
			),
		});
	};

	const concurrency = Math.max(
		1,
		Math.min(3, Math.floor(options.concurrency ?? 3)),
	);
	const interval = Math.max(0, options.delayMs ?? 350);
	let nextRequest = 0;
	let failed = false;
	async function waitTurn(): Promise<boolean> {
		const current = Date.now();
		const scheduled = Math.max(current, nextRequest);
		nextRequest = scheduled + interval;
		const wait = scheduled - current;
		if (wait > 0) {
			await new Promise<void>(resolve => {
				const timer = setTimeout(done, wait);
				function done() {
					clearTimeout(timer);
					signal.removeEventListener('abort', done);
					resolve();
				}

				signal.addEventListener('abort', done, {once: true});
			});
		}

		return !signal.aborted && !failed;
	}

	async function fetchAccount(account: Account, id: string): Promise<void> {
		const feed = source.posts(id);
		let pages = 0;
		do {
			// Each page needs the preceding cursor and a rate-limited slot.
			// eslint-disable-next-line no-await-in-loop
			if (!(await waitTurn())) return;
			// eslint-disable-next-line no-await-in-loop
			const items = await feed.items();
			if (signal.aborted || failed) return;
			pages++;
			let foundPhoto = false;
			for (const item of items) {
				const post = toPhoto(item, account, cutoff.getTime(), now);
				if (post) {
					posts.set(post.id, post);
					foundPhoto = true;
				}
			}

			const times = items
				.map(item => item.taken_at)
				.filter(
					(time): time is number =>
						typeof time === 'number' && Number.isFinite(time),
				);
			if (
				times.length === items.length &&
				times.length > 0 &&
				times.every(time => time * 1000 < cutoff.getTime())
			)
				break;
			if (
				(items.length === 0 || pages >= (options.maxPagesPerAccount ?? 10)) &&
				feed.isMoreAvailable()
			) {
				state.incomplete = true;
				break;
			}

			// Publish before the next page; the final page is published with the account count.
			if (foundPhoto && feed.isMoreAvailable()) publish();
		} while (feed.isMoreAvailable());

		if (!signal.aborted && !failed) {
			state.accounts++;
			publish();
		}
	}

	try {
		const following = source.following();
		let followingPages = 0;
		do {
			if (signal.aborted || failed) break;
			if (++followingPages > 100) {
				state.incomplete = true;
				break;
			}

			// Cursor pages depend on the preceding response.
			// eslint-disable-next-line no-await-in-loop
			const accounts = await following.items();
			const queue: Array<{account: Account; id: string}> = [];
			for (const account of accounts) {
				const id =
					account.pk_id ??
					(typeof account.pk === 'number' && !Number.isSafeInteger(account.pk)
						? ''
						: String(account.pk));
				if (!/^\d{1,30}$/.test(id) || !/^[\w.]{1,30}$/.test(account.username)) {
					state.incomplete = true;
					continue;
				}

				if (users.has(id)) continue;
				users.add(id);
				queue.push({account, id});
			}

			let next = 0;
			// Workers intentionally share the failure flag and queue index.
			// eslint-disable-next-line @typescript-eslint/no-loop-func
			async function worker(): Promise<void> {
				while (!signal.aborted && !failed) {
					const entry = queue[next++];
					if (!entry) return;
					try {
						// eslint-disable-next-line no-await-in-loop
						await fetchAccount(entry.account, entry.id);
					} catch {
						failed = true;
					}
				}
			}

			// Each worker stops on the first failure; wait for outstanding requests before leaving.
			// eslint-disable-next-line no-await-in-loop
			await Promise.all(
				Array.from({length: Math.min(concurrency, queue.length)}, async () =>
					worker(),
				),
			);
			if (failed) throw new Error('calendar request failed');
			if (accounts.length === 0 && following.isMoreAvailable()) {
				state.incomplete = true;
				break;
			}
		} while (following.isMoreAvailable());

		state.status = signal.aborted ? 'cancelled' : 'complete';
	} catch {
		state.status = signal.aborted ? 'cancelled' : 'error';
		state.incomplete = true;
	}

	publish();
}
