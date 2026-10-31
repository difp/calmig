/* eslint-disable @typescript-eslint/no-unsafe-call */
import {Buffer} from 'node:buffer';
import http from 'node:http';
import type {AddressInfo} from 'node:net';
import process from 'node:process';
import test from 'ava';
import {CaaTransport} from '../source/utils/caa-transport.js';

test.serial('CAA CONNECT request does not contain the login form', async t => {
	const server = http.createServer();
	let path = '';
	let firstBytes = '';
	let authorization = '';
	server.on('connect', (request, socket, head) => {
		path = request.url ?? '';
		firstBytes = head.toString('utf8');
		authorization = request.headers['proxy-authorization'] ?? '';
		socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
	});
	await new Promise<void>(resolve => {
		server.listen(0, '127.0.0.1', resolve);
	});
	const {port} = server.address() as AddressInfo;
	const previous = {
		https: process.env['HTTPS_PROXY'],
		lower: process.env['https_proxy'],
		no: process.env['NO_PROXY'],
		noLower: process.env['no_proxy'],
	};
	process.env['HTTPS_PROXY'] = `http://proxy-user:proxy-pass@127.0.0.1:${port}`;
	process.env['https_proxy'] = `http://proxy-user:proxy-pass@127.0.0.1:${port}`;
	process.env['NO_PROXY'] = '';
	process.env['no_proxy'] = '';
	const transport = new CaaTransport('example.invalid');
	try {
		await t.throwsAsync(
			transport.post('/test', {secret: 'SENSITIVE_FORM'}, {}),
		);
		t.is(path, 'example.invalid:443');
		t.false(firstBytes.includes('SENSITIVE_FORM'));
		t.is(
			authorization,
			`Basic ${Buffer.from('proxy-user:proxy-pass').toString('base64')}`,
		);
	} finally {
		transport.close();
		await new Promise<void>(resolve => {
			server.close(() => {
				resolve();
			});
		});
		for (const [key, value] of [
			['HTTPS_PROXY', previous.https],
			['https_proxy', previous.lower],
			['NO_PROXY', previous.no],
			['no_proxy', previous.noLower],
		] as const) {
			if (value === undefined) Reflect.deleteProperty(process.env, key);
			else process.env[key] = value;
		}
	}
});

test.serial(
	'CAA respects NO_PROXY and bypasses the configured proxy',
	async t => {
		const server = http.createServer();
		let connected = false;
		server.on('connect', (_request, socket) => {
			connected = true;
			socket.destroy();
		});
		await new Promise<void>(resolve => {
			server.listen(0, '127.0.0.1', resolve);
		});
		const {port} = server.address() as AddressInfo;
		const previous = {
			https: process.env['HTTPS_PROXY'],
			lower: process.env['https_proxy'],
			no: process.env['NO_PROXY'],
			noLower: process.env['no_proxy'],
		};
		process.env['HTTPS_PROXY'] = `http://127.0.0.1:${port}`;
		process.env['https_proxy'] = `http://127.0.0.1:${port}`;
		process.env['NO_PROXY'] = '127.0.0.1';
		process.env['no_proxy'] = '127.0.0.1';
		const transport = new CaaTransport('127.0.0.1:1');
		try {
			await t.throwsAsync(transport.post('/test', {}, {}));
			t.false(connected);
		} finally {
			transport.close();
			await new Promise<void>(resolve => {
				server.close(() => {
					resolve();
				});
			});
			for (const [key, value] of [
				['HTTPS_PROXY', previous.https],
				['https_proxy', previous.lower],
				['NO_PROXY', previous.no],
				['no_proxy', previous.noLower],
			] as const) {
				if (value === undefined) Reflect.deleteProperty(process.env, key);
				else process.env[key] = value;
			}
		}
	},
);
