const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { JSDOM } = require("jsdom");
const script = fs.readFileSync(
	path.join(__dirname, "../src-tauri/src/web/following-snapshot.js"),
	"utf8",
);
const root = "xdt_api__v1__feed__timeline__connection";
function snapshot(
	value,
	url = "https://www.instagram.com/?variant=following",
	expected = "123456",
) {
	const dom = new JSDOM("<!doctype html>", { url, runScripts: "outside-only" });
	try {
		dom.window.document.cookie = "ds_user_id=123456";
		const el = dom.window.document.createElement("script");
		el.type = "application/json";
		el.textContent = JSON.stringify(value);
		dom.window.document.body.append(el);
		return JSON.parse(dom.window.eval(script)(expected));
	} finally {
		dom.window.close();
	}
}
function fixture(variant = "following") {
	return {
		items: [
			{
				queryName: "PolarisFeedTimelineRootV2Query",
				queryID: "123456",
				variables: { variant, first: 3, data: { device_id: "PRIVATE_DEVICE" } },
			},
			{
				[root]: {
					edges: [
						{
							node: {
								media: {
									code: "ABC",
									taken_at: 100,
									media_type: 1,
									user: {
										pk: "7",
										username: "friend",
										friendship_status: { following: true },
										email: "PRIVATE_EMAIL",
									},
									caption: { text: "hello" },
									image_versions2: {
										candidates: [
											{
												url: "https://scontent.cdninstagram.com/a.jpg",
												width: 320,
											},
										],
									},
									logging_info_token: "PRIVATE_TOKEN",
								},
							},
						},
					],
					page_info: { has_next_page: true, end_cursor: "cursor" },
				},
			},
		],
	};
}
test("captures only fields needed to filter a verified Following response", () => {
	const result = snapshot(fixture());
	assert.equal(result.status, "ready");
	assert.equal(
		result.data[root].edges[0].node.media.user.friendship_status.following,
		true,
	);
	assert.equal(result.data[root].edges[0].node.media.caption.text, "hello");
	assert.equal(result.data[root].page_info.has_next_page, true);
	assert.doesNotMatch(JSON.stringify(result), /PRIVATE_/);
});
test("rejects wrong mode, ambiguous response, and other origins", () => {
	assert.equal(snapshot(fixture("home")).status, "invalid");
	const multiple = fixture();
	multiple.items.push(multiple.items[1]);
	assert.equal(snapshot(multiple).status, "invalid");
	assert.equal(
		snapshot(fixture(), "https://example.com/?variant=following").status,
		"wrong_page",
	);
	assert.equal(
		snapshot(fixture(), undefined, "999999").status,
		"wrong_account",
	);
});
test("marks unknown and advertising nodes for conservative filtering", () => {
	const value = fixture();
	value.items[1][root].edges.push(
		{ node: { ad: { tracking: "PRIVATE_TOKEN" } } },
		{ node: { new_union: { secret: "PRIVATE_TOKEN" } } },
	);
	const result = snapshot(value);
	assert.equal(result.data[root].edges[1].node.ad, true);
	assert.equal(result.data[root].edges[2].node.unknown, true);
	assert.doesNotMatch(JSON.stringify(result), /PRIVATE_/);
});
