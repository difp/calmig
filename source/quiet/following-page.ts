type Account = {pk: string | number; pk_id?: string; username: string};
type FollowingResponse = {users?: unknown; next_max_id?: unknown};

/** Request larger pages, but always respect the cursor returned by Instagram. */
export function followingPages(
	fetchPage: (
		cursor: string | undefined,
		count: number,
	) => Promise<FollowingResponse>,
) {
	let cursor: string | undefined;
	let more = true;
	const seen = new Set<string>();
	return {
		async items(): Promise<Account[]> {
			const page = await fetchPage(cursor, 200);
			if (!Array.isArray(page.users)) throw new Error('invalid_following_page');
			const next = page.next_max_id;
			if (
				next !== undefined &&
				next !== null &&
				next !== '' &&
				(typeof next !== 'string' || next.length > 2048 || seen.has(next))
			)
				throw new Error('invalid_following_cursor');
			cursor = typeof next === 'string' && next ? next : undefined;
			more = Boolean(cursor);
			if (cursor) seen.add(cursor);
			return page.users as Account[];
		},
		isMoreAvailable: () => more,
	};
}
