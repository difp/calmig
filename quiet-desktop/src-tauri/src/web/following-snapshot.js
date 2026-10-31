// Executed inside the official Instagram WebView; it returns no session data.
(expectedUserId) => {
	const locationOk =
		location.protocol === "https:" &&
		location.hostname === "www.instagram.com" &&
		location.pathname === "/" &&
		new URLSearchParams(location.search).get("variant") === "following";
	if (!locationOk) return JSON.stringify({ status: "wrong_page" });
	const browserUser = document.cookie
		.split(";")
		.map((part) => part.trim())
		.find((part) => part.startsWith("ds_user_id="))
		?.slice("ds_user_id=".length);
	if (!/^\d{1,30}$/.test(expectedUserId) || browserUser !== expectedUserId)
		return JSON.stringify({ status: "wrong_account" });
	const connectionKey = "xdt_api__v1__feed__timeline__connection";
	const operation = "PolarisFeedTimelineRootV2Query";
	const unionKeys = [
		"ad",
		"explore_story",
		"end_of_feed_demarcator",
		"stories_netego",
		"suggested_users",
		"bloks_netego",
		"abra_icebreakers_in_feed_unit",
		"ad4ad_in_webfeed",
	];
	let budget = 200_000;
	let roots = 0;
	let followingRoots = 0;
	const connections = [];
	const pending = [];
	for (const script of document.querySelectorAll(
		'script[type="application/json"]',
	)) {
		if (script.textContent.length > 4_000_000)
			return JSON.stringify({ status: "invalid" });
		try {
			pending.push(JSON.parse(script.textContent));
		} catch {}
	}
	while (pending.length) {
		if (--budget < 0) return JSON.stringify({ status: "invalid" });
		const value = pending.pop();
		if (!value || typeof value !== "object") continue;
		if (!Array.isArray(value)) {
			if (value.queryName === operation) {
				roots++;
				const vars = value.variables;
				if (
					/^\d{1,30}$/.test(String(value.queryID ?? "")) &&
					vars?.variant === "following" &&
					vars.data &&
					typeof vars.data === "object" &&
					Number.isSafeInteger(vars.first) &&
					vars.first > 0 &&
					vars.first <= 100
				)
					followingRoots++;
			}
			const connection = value[connectionKey];
			if (
				connection &&
				typeof connection === "object" &&
				!Array.isArray(connection)
			)
				connections.push(connection);
		}
		for (const child of Object.values(value))
			if (child && typeof child === "object") pending.push(child);
	}
	if (roots === 0 || connections.length === 0)
		return JSON.stringify({ status: "pending" });
	if (roots !== 1 || followingRoots !== 1 || connections.length !== 1)
		return JSON.stringify({ status: "invalid" });
	const connection = connections[0];
	if (
		!Array.isArray(connection.edges) ||
		connection.edges.length > 100 ||
		!connection.page_info ||
		typeof connection.page_info.has_next_page !== "boolean"
	)
		return JSON.stringify({ status: "invalid" });
	const safeImage = (item) => ({
		media_type: item?.media_type,
		video_versions: item?.video_versions == null ? null : true,
		has_audio: item?.has_audio,
		image_versions2: {
			candidates: Array.isArray(item?.image_versions2?.candidates)
				? item.image_versions2.candidates
						.slice(0, 20)
						.map((c) => ({ url: c?.url, width: c?.width }))
				: [],
		},
	});
	const safeMedia = (media) => ({
		...safeImage(media),
		code: media.code,
		taken_at: media.taken_at,
		product_type: media.product_type,
		is_sponsored: media.is_sponsored,
		is_recommended: media.is_recommended,
		is_paid_partnership: media.is_paid_partnership,
		music_metadata: media.music_metadata == null ? null : true,
		caption: { text: media.caption?.text },
		user: {
			pk: media.user?.pk,
			id: media.user?.id,
			username: media.user?.username,
			friendship_status: {
				following: media.user?.friendship_status?.following,
			},
		},
		carousel_media: Array.isArray(media.carousel_media)
			? media.carousel_media.slice(0, 21).map(safeImage)
			: null,
	});
	const edges = connection.edges.map((edge) => {
		const node = edge?.node;
		if (!node || typeof node !== "object" || Array.isArray(node))
			return { node: null };
		const safe = {};
		for (const [key, value] of Object.entries(node)) {
			if (value == null || key === "__typename") continue;
			if (key === "media" && typeof value === "object" && !Array.isArray(value))
				safe.media = safeMedia(value);
			else if (unionKeys.includes(key)) safe[key] = true;
			else safe.unknown = true;
		}
		return { node: safe };
	});
	const value = {
		status: "ready",
		data: {
			[connectionKey]: {
				edges,
				page_info: {
					has_next_page: connection.page_info.has_next_page,
					end_cursor: connection.page_info.end_cursor,
				},
			},
		},
	};
	const output = JSON.stringify(value);
	return output.length <= 4_000_000
		? output
		: JSON.stringify({ status: "invalid" });
}
