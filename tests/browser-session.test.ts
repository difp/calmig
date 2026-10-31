/* eslint-disable @typescript-eslint/no-unsafe-call */
import {Buffer} from 'node:buffer';
import test from 'ava';
import {IgApiClient} from 'instagram-private-api';
import {attachBrowserCookies} from '../source/quiet/browser-session.js';

test('official browser session imports only the required safe cookies', t => {
	const ig = new IgApiClient();
	const id = attachBrowserCookies(ig, [
		{name: 'sessionid', value: '123456%3Asynthetic'},
		{name: 'ds_user_id', value: '123456'},
		{name: 'csrftoken', value: 'synthetic'},
		{name: 'other', value: 'must-not-import'},
	]);
	t.is(id, '123456');
	t.is(ig.state.cookieUserId, '123456');
	t.is(ig.state.cookieCsrfToken, 'synthetic');
	const header = String(ig.state.authorization ?? '');
	t.true(header.startsWith('Bearer IGT:2:'));
	const decoded: unknown = JSON.parse(
		Buffer.from(header.slice('Bearer IGT:2:'.length), 'base64').toString(
			'utf8',
		),
	);
	t.deepEqual(decoded, {
		ds_user_id: '123456',
		sessionid: '123456%3Asynthetic',
		should_use_header_over_cookies: true,
	});
	t.falsy(ig.state.extractCookie('other'));
});

test('invalid or incomplete browser cookies are rejected', t => {
	const ig = new IgApiClient();
	t.throws(() =>
		attachBrowserCookies(ig, [
			{name: 'sessionid', value: 'x; injected=1'},
			{name: 'ds_user_id', value: '123'},
			{name: 'csrftoken', value: 'x'},
		]),
	);
	t.throws(() => attachBrowserCookies(ig, [{name: 'sessionid', value: 'x'}]));
	t.throws(() =>
		attachBrowserCookies(ig, [
			{name: 'sessionid', value: '456%3Atest'},
			{name: 'ds_user_id', value: '123'},
			{name: 'csrftoken', value: 'x'},
		]),
	);
});

test('the local session stays usable after transferring between client instances', async t => {
	const original = new IgApiClient();
	attachBrowserCookies(original, [
		{name: 'sessionid', value: '123456%3Asynthetic'},
		{name: 'ds_user_id', value: '123456'},
		{name: 'csrftoken', value: 'synthetic'},
	]);
	const saved = await original.state.serialize();
	const restored = new IgApiClient();
	await restored.state.deserialize(saved);
	t.is(restored.state.cookieUserId, '123456');
	t.true(restored.state.authorization?.startsWith('Bearer IGT:2:'));
});
