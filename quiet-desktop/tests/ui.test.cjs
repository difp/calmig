const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {JSDOM} = require('jsdom');
const html = fs.readFileSync(path.join(__dirname, '../ui/index.html'), 'utf8');
const script = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const i18nScript = fs.readFileSync(
	path.join(__dirname, '../ui/i18n.js'),
	'utf8',
);
const tick = () => new Promise(resolve => setTimeout(resolve, 30));
function setup({
	verification = false,
	reason = '',
	restored = false,
	photos,
	commentFailure = false,
	unchangedStatus = false,
	language,
} = {}) {
	const dom = new JSDOM(html, {
		url: 'http://tauri.localhost/',
		runScripts: 'outside-only',
	});
	const {window} = dom;
	if (language) window.localStorage.setItem('calmig-language', language);
	window.HTMLDialogElement.prototype.showModal = function () {
		this.open = true;
	};
	window.HTMLDialogElement.prototype.close = function () {
		this.open = false;
	};
	const calls = [];
	window.__TAURI__ = {
		core: {
			invoke: async (command, payload = {}) => {
				if (command === 'restore_instagram_login') {
					calls.push({action: command});
					return restored ? {connected: true, username: 'sample.user'} : null;
				}
				if (
					command === 'open_instagram_login' ||
					command === 'clear_instagram_login'
				) {
					calls.push({action: command});
					return null;
				}
				if (command === 'complete_instagram_login') {
					calls.push({action: command});
					return {connected: true, username: 'sample.user'};
				}
				const {request} = payload;
				calls.push(request);
				switch (request.action) {
					case 'login':
						return reason
							? {connected: false, reason}
							: verification
								? {verification: true}
								: {connected: true, username: request.username};
					case 'code':
						return {connected: true, username: 'sample.user'};
					case 'cancel-login':
						return {cancelled: true};
					case 'calendar-start':
						return {started: true};
					case 'photo-details':
						return {
							caption: 'Подпись из Instagram',
							comments: [
								{username: 'friend', text: 'Комментарий автора'},
								{username: 'other', text: 'Другой комментарий'},
							],
							more: true,
						};
					case 'photo-comments-more':
						return {
							comments: [{username: 'next', text: 'Ещё комментарий'}],
							more: false,
						};
					case 'photo-comment-send':
						if (commentFailure) throw Error('network');
						return {sent: true, username: 'sample.user'};
					case 'profile-open':
						return {
							username: request.username,
							name: 'Друг',
							bio: 'О себе',
							photos: photos ?? [],
							more: true,
						};
					case 'profile-more':
						return {photos: [], more: false};
					case 'calendar-status':
						if (unchangedStatus && request.revision === 1)
							return {unchanged: true, revision: 1};
						return {
							revision: unchangedStatus ? 1 : undefined,
							status: 'complete',
							accounts: 1,
							incomplete: false,
							photos: photos ?? [
								{
									id: 'one',
									username: 'friend',
									thumbnail: 'https://scontent.cdninstagram.com/a.jpg',
									images: ['https://scontent.cdninstagram.com/a.jpg'],
									publishedAt: new Date().toISOString(),
								},
							],
						};
					case 'threads':
						return request.more
							? {threads: [{id: '2', title: 'Ещё друг'}], hasMore: false}
							: {threads: [{id: '1', title: 'Друг'}], hasMore: true};
					case 'messages':
						return request.cursor
							? {
									messages: [{id: '0', text: 'Раньше', mine: false}],
									cursor: null,
								}
							: {
									messages: [{id: '1', text: 'Привет', mine: false}],
									cursor: 'cursor1',
								};
					case 'send':
						return {sent: true};
					default:
						throw Error('unexpected request');
				}
			},
		},
	};
	window.eval(i18nScript);
	window.eval(script);
	return {dom, window, calls};
}
test('restores a saved browser login on startup', async () => {
	const {dom, window} = setup({restored: true});
	try {
		await tick();
		assert.equal(window.document.title, 'CalmIG');
		assert.equal(
			window.document.querySelector('.brand').getAttribute('aria-label'),
			'CalmIG',
		);
		assert(window.document.querySelectorAll('.brand svg circle').length > 0);
		assert.equal(window.document.querySelector('#login').open, false);
		assert.equal(
			window.document.querySelector('#profile-name').textContent,
			'@sample.user',
		);
	} finally {
		dom.window.close();
	}
});
test('switches languages and preserves user-generated text', async () => {
	const photo = {
		id: 'one',
		username: 'friend',
		caption: 'Сегодня',
		thumbnail: 'https://scontent.cdninstagram.com/a.jpg',
		images: ['https://scontent.cdninstagram.com/a.jpg'],
		publishedAt: new Date().toISOString(),
	};
	const {dom, window} = setup({restored: true, photos: [photo]});
	try {
		await tick();
		window.document.querySelector('.tile').click();
		const select = window.document.querySelector('.language-select');
		select.value = 'en';
		select.dispatchEvent(new window.Event('change', {bubbles: true}));
		assert.equal(window.document.documentElement.lang, 'en');
		assert.equal(window.localStorage.getItem('calmig-language'), 'en');
		assert.equal(
			window.document.querySelector('#photo-caption').textContent,
			'Сегодня',
		);
		assert.equal(
			window.document.querySelector('#photo-profile').textContent,
			'Open profile',
		);
		await tick();
		assert.equal(
			window.document.querySelector('#photo-comments-more').textContent,
			'More comments',
		);
		select.value = 'ru';
		select.dispatchEvent(new window.Event('change', {bubbles: true}));
		assert.equal(
			window.document.querySelector('#photo-profile').textContent,
			'Открыть профиль',
		);
	} finally {
		dom.window.close();
	}
});
test('restores the saved English language on launch', async () => {
	const {dom, window} = setup({language: 'en'});
	try {
		assert.equal(window.document.documentElement.lang, 'en');
		assert.equal(
			window.document.querySelector('#browser-open').textContent,
			'Open Instagram',
		);
		assert.equal(window.document.querySelector('.language-select').value, 'en');
		const text = window.document
			.querySelector('#login')
			.textContent.replaceAll('Русский', '');
		assert.doesNotMatch(text, /[А-Яа-яЁё]/);
		assert.doesNotMatch(
			window.document.body.textContent.replaceAll('Русский', ''),
			/[А-Яа-яЁё]/,
		);
	} finally {
		dom.window.close();
	}
});
test('publishes a comment only after submission and shows it in the discussion', async () => {
	const {dom, window, calls} = setup({restored: true});
	try {
		await tick();
		window.document.querySelector('.tile').click();
		await tick();
		assert(!calls.some(call => call.action === 'photo-comment-send'));
		window.document.querySelector('#photo-comment-input').value = 'Спасибо!';
		window.document
			.querySelector('#photo-comment-form')
			.dispatchEvent(
				new window.Event('submit', {bubbles: true, cancelable: true}),
			);
		await tick();
		assert(
			calls.some(
				call =>
					call.action === 'photo-comment-send' &&
					call.code === 'one' &&
					call.text === 'Спасибо!',
			),
		);
		assert.match(
			window.document.querySelector('#photo-comments').textContent,
			/@sample.user Спасибо!/,
		);
		assert.equal(
			window.document.querySelector('#photo-comment-input').value,
			'',
		);
	} finally {
		dom.window.close();
	}
});
test('keeps the comment text when sending fails', async () => {
	const {dom, window} = setup({restored: true, commentFailure: true});
	try {
		await tick();
		window.document.querySelector('.tile').click();
		await tick();
		window.document.querySelector('#photo-comment-input').value = 'Черновик';
		window.document
			.querySelector('#photo-comment-form')
			.dispatchEvent(
				new window.Event('submit', {bubbles: true, cancelable: true}),
			);
		await tick();
		assert.equal(
			window.document.querySelector('#photo-comment-input').value,
			'Черновик',
		);
		assert.match(
			window.document.querySelector('#photo-comment-feedback').textContent,
			/не подтверждена/,
		);
	} finally {
		dom.window.close();
	}
});
test('groups each day by author while preserving their photo order', async () => {
	const photo = (id, username) => ({
		id,
		username,
		thumbnail: 'https://scontent.cdninstagram.com/a.jpg',
		images: ['https://scontent.cdninstagram.com/a.jpg'],
		publishedAt: new Date().toISOString(),
	});
	const {dom, window} = setup({
		restored: true,
		photos: [photo('a', 'first'), photo('b', 'second'), photo('c', 'first')],
	});
	try {
		await tick();
		const groups = window.document.querySelectorAll(
			'.day:first-child .day-author',
		);
		assert.equal(groups.length, 2);
		assert.equal(groups[0].querySelector('h3').textContent, '@first');
		assert.equal(groups[0].querySelectorAll('.tile').length, 2);
		assert.equal(groups[1].querySelector('h3').textContent, '@second');
		groups[1].querySelector('.tile').click();
		assert.equal(
			window.document.querySelector('#photo-author').textContent,
			'@second',
		);
		await tick();
	} finally {
		dom.window.close();
	}
});
test('keeps existing tiles and focus when new photos arrive', async () => {
	const photo = (id, username) => ({
		id,
		username,
		thumbnail: 'https://scontent.cdninstagram.com/a.jpg',
		images: ['https://scontent.cdninstagram.com/a.jpg'],
		publishedAt: new Date().toISOString(),
	});
	const photos = [photo('a', 'first')];
	const {dom, window} = setup({restored: true, photos});
	try {
		await tick();
		const tile = window.document.querySelector('.tile');
		tile.focus();
		photos.push(photo('b', 'second'));
		window.document.querySelector('#refresh').click();
		await tick();
		assert.equal(window.document.querySelector('.tile'), tile);
		assert.equal(window.document.activeElement, tile);
		assert.equal(window.document.querySelectorAll('.tile').length, 2);
	} finally {
		dom.window.close();
	}
});
test('keeps the calendar when the adapter reports no changes', async () => {
	const {dom, window, calls} = setup({restored: true, unchangedStatus: true});
	try {
		await tick();
		const tile = window.document.querySelector('.tile');
		window.document.querySelector('#refresh').click();
		await tick();
		assert.equal(window.document.querySelector('.tile'), tile);
		assert(
			calls.some(
				call => call.action === 'calendar-status' && call.revision === 1,
			),
		);
	} finally {
		dom.window.close();
	}
});
test('shows the caption as text and opens the selected author profile', async () => {
	const photo = {
		id: 'a',
		username: 'friend',
		caption: '<b>Подпись</b>',
		thumbnail: 'https://scontent.cdninstagram.com/a.jpg',
		images: ['https://scontent.cdninstagram.com/a.jpg'],
		publishedAt: new Date().toISOString(),
	};
	const {dom, window, calls} = setup({restored: true, photos: [photo]});
	try {
		await tick();
		window.document.querySelector('.tile').click();
		const caption = window.document.querySelector('#photo-caption');
		assert.equal(caption.textContent, '<b>Подпись</b>');
		assert.equal(caption.querySelector('b'), null);
		window.document.querySelector('#photo-author').click();
		await tick();
		assert(
			calls.some(
				call => call.action === 'profile-open' && call.username === 'friend',
			),
		);
		assert.equal(window.document.querySelector('#author-profile').open, true);
		assert.equal(
			window.document.querySelector('#author-profile-name').textContent,
			'@friend',
		);
		assert.equal(
			window.document.querySelectorAll('#author-profile-grid .tile').length,
			1,
		);
		window.document.querySelector('#author-profile-more').click();
		await tick();
		assert(calls.some(call => call.action === 'profile-more'));
	} finally {
		dom.window.close();
	}
});
test('loads a caption and author comments for a previously cached photo', async () => {
	const {dom, window, calls} = setup({restored: true});
	try {
		await tick();
		window.document.querySelector('.tile').click();
		await tick();
		assert.equal(
			window.document.querySelector('#photo-caption').textContent,
			'Подпись из Instagram',
		);
		assert.match(
			window.document.querySelector('#photo-comments').textContent,
			/Комментарий автора/,
		);
		assert.match(
			window.document.querySelector('#photo-comments').textContent,
			/@other Другой комментарий/,
		);
		window.document.querySelector('#photo-comments-more').click();
		await tick();
		assert.match(
			window.document.querySelector('#photo-comments').textContent,
			/@next Ещё комментарий/,
		);
		assert.match(
			window.document.querySelector('#photo-profile').textContent,
			/профил/i,
		);
		assert(
			calls.some(
				call => call.action === 'photo-details' && call.code === 'one',
			),
		);
	} finally {
		dom.window.close();
	}
});
test('a failed login remains on the form and displays a safe reason', async () => {
	const {dom, window} = setup({reason: 'network'});
	try {
		const doc = window.document;
		doc.querySelector('#manual-open').click();
		doc.querySelector('#username').value = 'sample.user';
		doc.querySelector('#password').value = 'test-password';
		const form = doc.querySelector('#login-form');
		form.dispatchEvent(
			new window.SubmitEvent('submit', {
				bubbles: true,
				cancelable: true,
				submitter: form.querySelector('[type=submit]'),
			}),
		);
		await tick();
		assert.equal(doc.querySelector('#login').open, true);
		assert.equal(doc.querySelector('#password').value, '');
		assert.match(doc.querySelector('#login-status').textContent, /сеть/i);
		assert(!doc.body.textContent.includes('test-password'));
	} finally {
		dom.window.close();
	}
});
test('login opens seven-day photo grid and message sending uses the chosen dialog', async () => {
	const {dom, window, calls} = setup();
	try {
		const doc = window.document;
		doc.querySelector('#manual-open').click();
		assert.equal(doc.querySelectorAll('.day').length, 7);
		doc.querySelector('#username').value = 'sample.user';
		doc.querySelector('#password').value = 'test-password';
		const form = doc.querySelector('#login-form');
		form.dispatchEvent(
			new window.SubmitEvent('submit', {
				bubbles: true,
				cancelable: true,
				submitter: form.querySelector('[type=submit]'),
			}),
		);
		await tick();
		assert.equal(doc.querySelector('#password').value, '');
		assert.equal(doc.querySelectorAll('.tile').length, 1);
		assert.equal(doc.querySelectorAll('.nav svg').length, 3);
		doc.querySelector('[data-view=messages]').click();
		await tick();
		doc.querySelector('.thread').click();
		await tick();
		assert.match(doc.querySelector('#chat').textContent, /Привет/);
		doc.querySelector('#chat > button').click();
		await tick();
		assert.match(doc.querySelector('#chat').textContent, /Раньше/);
		doc.querySelector('.more-threads').click();
		await tick();
		assert.match(doc.querySelector('#threads').textContent, /Ещё друг/);
		const composer = doc.querySelector('.composer');
		composer.querySelector('input').value = 'Ответ';
		composer.dispatchEvent(
			new window.SubmitEvent('submit', {
				bubbles: true,
				cancelable: true,
				submitter: composer.querySelector('button'),
			}),
		);
		await tick();
		assert(
			calls.some(
				call =>
					call.action === 'send' &&
					call.thread === '1' &&
					call.text === 'Ответ',
			),
		);
		assert.match(doc.querySelector('#chat').textContent, /Отправлено/);
	} finally {
		dom.window.close();
	}
});

test('verification can be cancelled and returns to the login form', async () => {
	const {dom, window, calls} = setup({verification: true});
	try {
		const doc = window.document;
		doc.querySelector('#manual-open').click();
		doc.querySelector('#username').value = 'sample.user';
		doc.querySelector('#password').value = 'test-password';
		const form = doc.querySelector('#login-form');
		form.dispatchEvent(
			new window.SubmitEvent('submit', {
				bubbles: true,
				cancelable: true,
				submitter: form.querySelector('[type=submit]'),
			}),
		);
		await tick();
		assert.equal(doc.querySelector('#code-form').hidden, false);
		doc.querySelector('#back-login').click();
		await tick();
		assert.equal(form.hidden, false);
		assert(calls.some(call => call.action === 'cancel-login'));
	} finally {
		dom.window.close();
	}
});

test('official login is opened separately and no cookies reach the UI', async () => {
	const {dom, window, calls} = setup();
	try {
		const doc = window.document;
		assert.equal(doc.querySelector('#login-form').hidden, true);
		doc.querySelector('#browser-open').click();
		await tick();
		assert.equal(doc.querySelector('#browser-complete').hidden, false);
		doc.querySelector('#browser-complete').click();
		await tick();
		assert.equal(doc.querySelector('#login').open, false);
		assert.equal(doc.querySelectorAll('.tile').length, 1);
		assert(calls.some(call => call.action === 'open_instagram_login'));
		assert(calls.some(call => call.action === 'complete_instagram_login'));
		assert(!JSON.stringify(calls).includes('sessionid'));
	} finally {
		dom.window.close();
	}
});
