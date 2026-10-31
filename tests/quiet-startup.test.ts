/* eslint-disable @typescript-eslint/no-unsafe-call */
import test from 'ava';
import {InstagramClient} from '../source/client.js';

test('quiet login setup does not request a timeline, reels, or realtime connection', async t => {
	const instance = new InstagramClient('sample.user');
	let feedRequests = 0;
	let realtimeRequests = 0;
	let sessionWrites = 0;
	const internal = instance as unknown as {
		configManager: {
			get(key: string): string;
			set(key: string, value: string): Promise<void>;
		};
		saveSessionState(): Promise<void>;
		postLoginFlow(): Promise<boolean>;
		initializeRealtime(): Promise<void>;
		postLoginSetup(
			username: string,
			options: {initializeRealtime: boolean; initializeFeed: boolean},
		): Promise<void>;
	};
	internal.saveSessionState = async () => {
		sessionWrites++;
	};

	internal.postLoginFlow = async () => {
		feedRequests++;
		return true;
	};

	internal.initializeRealtime = async () => {
		realtimeRequests++;
	};

	internal.configManager = {get: () => 'sample.user', async set() {}};
	await internal.postLoginSetup('sample.user', {
		initializeRealtime: false,
		initializeFeed: false,
	});
	t.is(sessionWrites, 1);
	t.is(feedRequests, 0);
	t.is(realtimeRequests, 0);
});
