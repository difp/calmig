/* eslint-disable @typescript-eslint/no-unsafe-call */
import test from 'ava';
import {IgApiClient} from 'instagram-private-api';
import {enforceTls} from '../source/quiet/secure-api.js';

test('quiet API always enables certificate validation, even if upstream requests disable it', async t => {
	const client = new IgApiClient();
	let strictSsl: boolean | undefined;
	client.request.send = async options => {
		strictSsl = options.strictSSL;
		throw new Error('test interception');
	};

	enforceTls(client);
	await t.throwsAsync(client.request.send({url: '/example', strictSSL: false}));
	t.true(strictSsl);
});
