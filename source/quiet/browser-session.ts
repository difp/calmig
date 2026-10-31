import {Buffer} from 'node:buffer';
import type {IgApiClient} from 'instagram-private-api';
import {applyCurrentDevice} from '../utils/caa-login.js';
import {enforceTls} from './secure-api.js';

export type BrowserCookie = {name: string; value: string};

/** Transfer only required cookies locally. No cookie values are returned or logged. */
export function attachBrowserCookies(
	client: IgApiClient,
	cookies: BrowserCookie[],
): string {
	if (!Array.isArray(cookies)) throw new Error('invalid_browser_session');
	const values = new Map<string, string>();
	for (const cookie of cookies) {
		if (
			!cookie ||
			!['sessionid', 'ds_user_id', 'csrftoken'].includes(cookie.name)
		)
			continue;
		if (
			values.has(cookie.name) ||
			typeof cookie.value !== 'string' ||
			cookie.value.length === 0 ||
			cookie.value.length > 4096 ||
			[...cookie.value].some(character => {
				const code = character.codePointAt(0) ?? 0;
				return (
					code <= 32 || code === 127 || character === ';' || character === ','
				);
			})
		)
			throw new Error('invalid_browser_session');
		values.set(cookie.name, cookie.value);
	}

	const userId = values.get('ds_user_id');
	const sessionId = values.get('sessionid');
	if (
		!userId ||
		!/^\d{1,30}$/.test(userId) ||
		!sessionId ||
		/^\d+/.exec(sessionId)?.[0] !== userId ||
		!values.has('csrftoken')
	)
		throw new Error('invalid_browser_session');
	client.state.generateDevice(userId);
	applyCurrentDevice(client, userId);
	enforceTls(client);
	const authorization = {
		ds_user_id: userId,
		sessionid: sessionId,
		should_use_header_over_cookies: true,
	};
	client.state.authorization = `Bearer IGT:2:${Buffer.from(JSON.stringify(authorization)).toString('base64')}`;
	for (const [name, value] of values)
		client.state.cookieJar.setCookie(
			`${name}=${value}; Domain=.instagram.com; Path=/; Secure`,
			'https://i.instagram.com/',
		);
	return userId;
}
