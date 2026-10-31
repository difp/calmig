/* eslint-disable @typescript-eslint/no-unsafe-call */

import os from 'node:os';
import fs from 'node:fs/promises';
import path from 'node:path';
import test from 'ava';
import {SessionManager} from '../source/session.js';
import {ConfigManager} from '../source/config.js';

// ── Helpers ──────────────────────────────────────────────────────────────────

const configManager = ConfigManager.getInstance();

async function withTempUsersDir(
	fn: (tmpDir: string) => Promise<void>,
): Promise<void> {
	const tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'ig-cli-test-'));
	const originalGet = configManager.get;

	try {
		configManager.get = ((key: string, fallback?: unknown) =>
			key === 'advanced.usersDir'
				? tmpDir
				: originalGet.call(
						configManager,
						key,
						fallback,
					)) as typeof configManager.get;
		await fn(tmpDir);
	} finally {
		configManager.get = originalGet;
		await fs.rm(tmpDir, {recursive: true, force: true});
	}
}

// ── Tests ─────────────────────────────────────────────────────────────────────

test('SessionManager rejects path traversal in account names', async t => {
	await Promise.all(
		['..', '../outside', 'folder/user', String.raw`folder\user`, 'a\0b'].map(
			async name => {
				const manager = new SessionManager(name);
				await t.throwsAsync(manager.saveSession({test: 'data'}), {
					message: 'Invalid username',
				});
				await t.throwsAsync(manager.sessionExists(), {
					message: 'Invalid username',
				});
			},
		),
	);
});

test.serial(
	'SessionManager: session file is written with owner-only permissions',
	async t => {
		await withTempUsersDir(async tmpDir => {
			const manager = new SessionManager('test-user');
			await manager.saveSession({test: 'data'});

			const sessionPath = path.join(tmpDir, 'test-user', 'session.ts.json');
			const stat = await fs.stat(sessionPath);

			t.is(
				stat.mode & 0o777,
				0o600,
				'Session file should be owner read/write only (0o600)',
			);
		});
	},
);

test.serial(
	'SessionManager: session directory is created with owner-only permissions',
	async t => {
		await withTempUsersDir(async tmpDir => {
			const manager = new SessionManager('test-user-dir');
			await manager.saveSession({test: 'data'});

			const dirPath = path.join(tmpDir, 'test-user-dir');
			const stat = await fs.stat(dirPath);

			t.is(
				stat.mode & 0o777,
				0o700,
				'Session directory should be owner-only (0o700)',
			);
		});
	},
);
