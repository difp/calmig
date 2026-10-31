/** Decode an Instagram shortcode without losing precision for large media IDs. */
export function mediaIdFromCode(code: string): string {
	const alphabet =
		'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
	if (!/^[\w-]{1,30}$/.test(code)) throw new Error('invalid_post');
	let id = 0n;
	for (const character of code)
		id = id * 64n + BigInt(alphabet.indexOf(character));
	return id.toString();
}

export function authorDetails(response: {
	// Instagram uses null for posts without a caption.
	// eslint-disable-next-line @typescript-eslint/no-restricted-types
	caption?: {text?: string} | null;
	comments?: Array<{
		text?: string;
		user?: {username?: string};
		preview_child_comments?: Array<{text?: string; user?: {username?: string}}>;
	}>;
}): {caption: string; comments: Array<{username: string; text: string}>} {
	const caption =
		typeof response.caption?.text === 'string'
			? response.caption.text.slice(0, 4000)
			: '';
	const comments = (response.comments ?? [])
		.flatMap(item => [item, ...(item.preview_child_comments ?? [])])
		.filter(
			item =>
				typeof item.user?.username === 'string' &&
				/^[\w.]{1,30}$/.test(item.user.username) &&
				typeof item.text === 'string',
		)
		.slice(0, 100)
		.map(item => ({
			username: item.user!.username!,
			text: item.text!.slice(0, 4000),
		}));
	return {caption, comments};
}
