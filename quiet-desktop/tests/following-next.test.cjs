const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { JSDOM } = require("jsdom");
const script = fs.readFileSync(
	path.join(__dirname, "../src-tauri/src/web/following-next.js"),
	"utf8",
);
const root = "xdt_api__v1__feed__timeline__connection";
function setup(variant = "following") {
	const dom = new JSDOM("<!doctype html>", {
		url: "https://www.instagram.com/?variant=following",
		runScripts: "outside-only",
	});
	const w = dom.window;
	w.document.cookie = "csrftoken=PRIVATE_CSRF";
	w.document.cookie = "ds_user_id=123456";
	w.require = (name) => (name === "LSD" ? { token: "PRIVATE_LSD" } : null);
	const el = w.document.createElement("script");
	el.type = "application/json";
	el.textContent = JSON.stringify({
		queryName: "PolarisFeedTimelineRootV2Query",
		queryID: "123",
		variables: { variant, first: 3, data: { device_id: "PRIVATE_DEVICE" } },
	});
	w.document.body.append(el);
	return { dom, w, run: w.eval("(" + script + ")") };
}
test("requests only Following and returns filtered fields, without tokens", async () => {
	const { dom, w, run } = setup();
	let request;
	w.fetch = async (url, options) => {
		request = { url, options };
		return {
			ok: true,
			text: async () =>
				JSON.stringify({
					data: {
						[root]: {
							edges: [
								{
									node: {
										media: {
											code: "ABC",
											taken_at: 1,
											media_type: 1,
											user: {
												pk: "7",
												username: "friend",
												friendship_status: { following: true },
												email: "PRIVATE_EMAIL",
											},
											image_versions2: { candidates: [] },
											logging_info_token: "PRIVATE_TOKEN",
										},
									},
								},
							],
							page_info: { has_next_page: false },
						},
					},
				}),
		};
	};
	try {
		const result = JSON.parse(await run("cursor-1", "123456"));
		assert.equal(result.status, "ready");
		const params = new URLSearchParams(request.options.body);
		const vars = JSON.parse(params.get("variables"));
		assert.equal(vars.variant, "following");
		assert.equal(vars.data.pagination_source, "following");
		assert.equal(vars.after, "cursor-1");
		assert.doesNotMatch(JSON.stringify(result), /PRIVATE_/);
	} finally {
		dom.window.close();
	}
});
test("does not issue a request for invalid cursor or wrong source", async () => {
	const { dom, w, run } = setup("home");
	let calls = 0;
	w.fetch = () => {
		calls++;
	};
	try {
		assert.equal(
			JSON.parse(await run("cursor", "123456")).status,
			"bootstrap_missing",
		);
		assert.equal(JSON.parse(await run("bad\n", "123456")).status, "invalid");
		assert.equal(
			JSON.parse(await run("cursor", "999999")).status,
			"wrong_account",
		);
		assert.equal(calls, 0);
	} finally {
		dom.window.close();
	}
});
test("rejects GraphQL errors and stops instead of retrying", async () => {
	const { dom, w, run } = setup();
	let calls = 0;
	w.fetch = async () => {
		calls++;
		return {
			ok: true,
			text: async () =>
				JSON.stringify({ errors: [{ message: "PRIVATE_SERVER" }] }),
		};
	};
	try {
		const result = await run("cursor", "123456");
		assert.equal(JSON.parse(result).status, "graphql_error");
		assert.equal(calls, 1);
		assert.doesNotMatch(result, /PRIVATE_/);
	} finally {
		dom.window.close();
	}
});
