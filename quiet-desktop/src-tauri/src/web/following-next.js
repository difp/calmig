// Called only in the official Following WebView. Cookies and tokens stay there.
async (cursor, expectedUserId) => {
	if (
		location.protocol !== "https:" ||
		location.hostname !== "www.instagram.com" ||
		location.pathname !== "/" ||
		new URLSearchParams(location.search).get("variant") !== "following"
	)
		return JSON.stringify({ status: "wrong_page" });
	const browserUser = document.cookie
		.split(";")
		.map((part) => part.trim())
		.find((part) => part.startsWith("ds_user_id="))
		?.slice("ds_user_id=".length);
	if (!/^\d{1,30}$/.test(expectedUserId) || browserUser !== expectedUserId)
		return JSON.stringify({ status: "wrong_account" });
	if (
		typeof cursor !== "string" ||
		!cursor ||
		cursor.length > 2048 ||
		/[\x00-\x1f\x7f]/.test(cursor)
	)
		return JSON.stringify({ status: "invalid" });
	let root = null,
		roots = 0,
		budget = 200_000;
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
		if (
			!Array.isArray(value) &&
			value.queryName === "PolarisFeedTimelineRootV2Query"
		) {
			roots++;
			if (
				/^\d{1,30}$/.test(String(value.queryID ?? "")) &&
				value.variables?.variant === "following" &&
				value.variables?.data &&
				typeof value.variables.data === "object"
			)
				root = value.variables;
		}
		for (const child of Object.values(value))
			if (child && typeof child === "object") pending.push(child);
	}
	if (roots !== 1 || !root)
		return JSON.stringify({ status: "bootstrap_missing" });
	let lsd;
	try {
		lsd = typeof require === "function" ? require("LSD")?.token : null;
	} catch {}
	const csrf = document.cookie
		.split(";")
		.map((part) => part.trim())
		.find((part) => part.startsWith("csrftoken="))
		?.slice("csrftoken=".length);
	if (typeof lsd !== "string" || !lsd)
		return JSON.stringify({ status: "lsd_missing" });
	if (typeof csrf !== "string" || !csrf)
		return JSON.stringify({ status: "csrf_missing" });
	const operation = "PolarisFeedRootPaginationCachedQuery_subscribe";
	const variables = {
		...root,
		after: cursor,
		before: null,
		first: 12,
		last: null,
		variant: "following",
		data: { ...root.data, pagination_source: "following" },
	};
	const body = new URLSearchParams({
		__a: "1",
		__d: "www",
		__comet_req: "7",
		lsd,
		fb_api_caller_class: "RelayModern",
		fb_api_req_friendly_name: operation,
		variables: JSON.stringify(variables),
		server_timestamps: "true",
		doc_id: "28851011141222448",
	});
	let response, result;
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), 10_000);
	try {
		response = await fetch("/graphql/query", {
			method: "POST",
			credentials: "same-origin",
			mode: "same-origin",
			redirect: "error",
			headers: {
				"content-type": "application/x-www-form-urlencoded",
				"x-ig-app-id": "936619743392459",
				"x-fb-friendly-name": operation,
				"x-fb-lsd": lsd,
				"x-csrftoken": csrf,
				"x-asbd-id": "129477",
			},
			body: body.toString(),
			signal: controller.signal,
		});
		if (!response.ok)
			return JSON.stringify({
				status:
					response.status === 429
						? "limited"
						: response.status >= 400 && response.status < 500
							? "http_client_error"
							: response.status >= 500
								? "http_server_error"
								: "unavailable",
			});
		const text = await response.text();
		if (text.length > 4_000_000) return JSON.stringify({ status: "invalid" });
		result = JSON.parse(text);
	} catch {
		return JSON.stringify({ status: "network_error" });
	} finally {
		clearTimeout(timer);
	}
	if (Array.isArray(result?.errors) && result.errors.length)
		return JSON.stringify({ status: "graphql_error" });
	const key = "xdt_api__v1__feed__timeline__connection";
	const connection = result?.data?.[key];
	if (
		!Array.isArray(connection?.edges) ||
		connection.edges.length > 100 ||
		!connection.page_info ||
		typeof connection.page_info.has_next_page !== "boolean"
	)
		return JSON.stringify({ status: "invalid" });
	const unions = [
		"ad",
		"explore_story",
		"end_of_feed_demarcator",
		"stories_netego",
		"suggested_users",
		"bloks_netego",
		"abra_icebreakers_in_feed_unit",
		"ad4ad_in_webfeed",
	];
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
			else if (unions.includes(key)) safe[key] = true;
			else safe.unknown = true;
		}
		return { node: safe };
	});
	const output = JSON.stringify({
		status: "ready",
		data: {
			[key]: {
				edges,
				page_info: {
					has_next_page: connection.page_info.has_next_page,
					end_cursor: connection.page_info.end_cursor,
				},
			},
		},
	});
	return output.length <= 4_000_000
		? output
		: JSON.stringify({ status: "invalid" });
}
