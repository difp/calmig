const {test} = require('node:test');
const assert = require('node:assert/strict');
const {spawn} = require('node:child_process');
const {createInterface} = require('node:readline');
const path = require('node:path');

test('local bridge rejects unauthenticated and malformed requests without printing secrets', async () => {
	const project = path.join(__dirname, '../..');
	const child = spawn(
		process.execPath,
		[path.join(project, 'dist/quiet-bridge.js')],
		{cwd: project, stdio: ['pipe', 'pipe', 'pipe']},
	);
	const lines = createInterface({input: child.stdout});
	const replies = [];
	const complete = new Promise((resolve, reject) => {
		const timeout = setTimeout(() => reject(new Error('bridge timeout')), 8000);
		lines.on('line', line => {
			replies.push(line);
			if (replies.length === 3) {
				clearTimeout(timeout);
				resolve();
			}
		});
		child.on('error', reject);
	});
	try {
		child.stdin.write('null\n');
		child.stdin.write(JSON.stringify({id: 1, action: 'status'}) + '\n');
		child.stdin.write(JSON.stringify({id: 2, action: 'threads'}) + '\n');
		child.stdin.end(
			JSON.stringify({
				id: 3,
				action: 'login',
				username: '../invalid',
				password: 'PRIVATE-TEST-SECRET',
			}) + '\n',
		);
		await complete;
		const parsed = replies
			.map(line => JSON.parse(line))
			.sort((a, b) => a.id - b.id);
		assert.deepEqual(parsed, [
			{id: 1, ok: true, data: {connected: false, username: ''}},
			{id: 2, ok: false},
			{id: 3, ok: false},
		]);
		assert(!replies.join('').includes('PRIVATE-TEST-SECRET'));
	} finally {
		lines.close();
		child.kill();
	}
});
