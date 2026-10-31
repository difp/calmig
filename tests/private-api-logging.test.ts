import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import test from 'ava';
import debug from 'debug';
import {ConfigManager} from '../source/config.js';
import {getLogger, initializeLogger} from '../source/utils/logger.js';

// AVA's assertion types are unavailable to the repository's lint configuration.
// eslint-disable-next-line @typescript-eslint/no-unsafe-call
test.serial(
	'API diagnostics do not expose private payloads in the log',
	async t => {
		const directory = await fs.mkdtemp(
			path.join(os.tmpdir(), 'ig-now-log-test-'),
		);
		const config = ConfigManager.getInstance();
		const originalInitialize = config.initialize;
		const originalGet = config.get;
		const originalLog = debug.log;
		const previousDebug = debug.disable();
		try {
			config.initialize = async () => {};
			config.get = (() => directory) as typeof config.get;
			await initializeLogger();
			const secret = 'PRIVATE_MESSAGE_AND_SESSION_TOKEN';
			debug('ig:private-test')('payload %s', secret);
			await delay(100);
			const logger = getLogger();
			const buffer = JSON.stringify(logger.getBufferedLogs());
			const file = await fs.readFile(logger.getLogFilePath(), 'utf8');
			// eslint-disable-next-line @typescript-eslint/no-unsafe-call
			t.false(buffer.includes(secret));
			// eslint-disable-next-line @typescript-eslint/no-unsafe-call
			t.false(file.includes(secret));
			// eslint-disable-next-line @typescript-eslint/no-unsafe-call
			t.true(buffer.includes('API diagnostics hidden'));
		} finally {
			config.initialize = originalInitialize;
			config.get = originalGet;
			debug.disable();
			debug.enable(previousDebug);
			debug.log = originalLog;
			await fs.rm(directory, {recursive: true, force: true});
		}
	},
);
