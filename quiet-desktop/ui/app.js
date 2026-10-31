'use strict';
const $ = selector => document.querySelector(selector);
const tr = window.CalmI18n.t;
const brandLetters = [
	['01110', '10001', '10000', '10000', '10000', '10001', '01110'],
	['00000', '00000', '01110', '00001', '01111', '10001', '01111'],
	['01100', '00100', '00100', '00100', '00100', '00100', '01110'],
	['00000', '00000', '11010', '10101', '10101', '10101', '10101'],
	['11111', '00100', '00100', '00100', '00100', '00100', '11111'],
	['01110', '10001', '10000', '10111', '10001', '10001', '01111'],
];
const brandSvg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
brandSvg.setAttribute('viewBox', '-0.5 -0.5 35 7');
brandSvg.setAttribute('aria-hidden', 'true');
brandLetters.forEach((letter, index) =>
	letter.forEach((row, y) =>
		[...row].forEach((dot, x) => {
			if (dot !== '1') return;
			const circle = document.createElementNS(
				'http://www.w3.org/2000/svg',
				'circle',
			);
			circle.setAttribute('cx', index * 6 + x);
			circle.setAttribute('cy', y);
			circle.setAttribute('r', '.41');
			brandSvg.append(circle);
		}),
	),
);
$('.brand').replaceChildren(brandSvg);
const dotIcons = {
	calendar: [
		'0100010',
		'1111111',
		'1000001',
		'1010101',
		'1000001',
		'1010101',
		'1111111',
	],
	messages: [
		'0111110',
		'1000001',
		'1010101',
		'1000001',
		'0111110',
		'0001000',
		'0001000',
	],
	profile: [
		'0011100',
		'0100010',
		'0100010',
		'0011100',
		'0111110',
		'1000001',
		'1111111',
	],
};
for (const button of document.querySelectorAll('.nav')) {
	const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
	svg.setAttribute('viewBox', '-1 -1 9 9');
	svg.setAttribute('aria-hidden', 'true');
	svg.classList.add('icon');
	dotIcons[button.dataset.view].forEach((row, y) =>
		[...row].forEach((dot, x) => {
			if (dot === '1') {
				const circle = document.createElementNS(
					'http://www.w3.org/2000/svg',
					'circle',
				);
				circle.setAttribute('cx', x);
				circle.setAttribute('cy', y);
				circle.setAttribute('r', '.42');
				svg.append(circle);
			}
		}),
	);
	button.querySelector('.icon').replaceWith(svg);
}
const {invoke} = window.__TAURI__.core;
const call = (action, extra = {}) =>
	invoke('bridge', {request: {action, ...extra}});
let connected = false,
	syncing = false,
	timer,
	posts = [],
	selectedThread = '',
	photoIndex = 0,
	currentPhoto,
	photoRequest = 0,
	profileRequest = 0,
	commentSending = false,
	calendarRevision,
	returnToAuthorProfile = false,
	renderStamp = '';
function element(tag, text, className) {
	const node = document.createElement(tag);
	if (text !== undefined) node.textContent = text;
	if (className) node.className = className;
	return node;
}
function showView(name) {
	document.querySelectorAll('.view').forEach(node => {
		node.hidden = node.id !== name;
	});
	document.querySelectorAll('.nav').forEach(node => {
		node.classList.toggle('active', node.dataset.view === name);
	});
}
document.querySelectorAll('.nav').forEach(button =>
	button.addEventListener('click', () => {
		showView(button.dataset.view);
		if (
			connected &&
			button.dataset.view === 'messages' &&
			!$('#threads').children.length
		)
			void loadThreads();
	}),
);
function setConnected(username) {
	connected = true;
	$('#profile-name').textContent = '@' + username;
	$('#disconnect').hidden = false;
	$('#clear-cache').hidden = false;
	$('#login').close();
	void refreshCalendar();
}
function showError(
	target,
	text = tr('Не удалось выполнить запрос. Проверьте уведомления Instagram.'),
) {
	target.textContent = text;
}
$('#browser-open').addEventListener('click', async event => {
	const button = event.currentTarget;
	button.disabled = true;
	try {
		await invoke('open_instagram_login', {
			language: window.CalmI18n.getLanguage(),
		});
		$('#browser-complete').hidden = false;
		$('#browser-status').textContent = tr(
			'Войдите в открывшемся окне, затем вернитесь сюда.',
		);
	} catch {
		showError($('#browser-status'), tr('Не удалось открыть страницу входа.'));
	} finally {
		button.disabled = false;
	}
});
$('#browser-complete').addEventListener('click', async event => {
	const button = event.currentTarget;
	button.disabled = true;
	$('#browser-open').disabled = true;
	$('#manual-open').disabled = true;
	$('#browser-status').textContent = tr('Подключаем аккаунт…');
	try {
		const result = await invoke('complete_instagram_login');
		if (!result.connected) throw new Error('not connected');
		setConnected(result.username);
	} catch {
		showError(
			$('#browser-status'),
			tr('Не удалось подключить сессию. Убедитесь, что вы вошли в Instagram.'),
		);
	} finally {
		button.disabled = false;
		$('#browser-open').disabled = false;
		$('#manual-open').disabled = false;
	}
});
$('#manual-open').addEventListener('click', () => {
	$('#browser-step').hidden = true;
	$('#login-form').hidden = false;
	$('#username').focus();
});
$('#manual-back').addEventListener('click', () => {
	$('#login-form').hidden = true;
	$('#browser-step').hidden = false;
	$('#browser-open').focus();
});
$('#login-form').addEventListener('submit', async event => {
	event.preventDefault();
	const button = event.submitter;
	button.disabled = true;
	$('#login-status').textContent = tr('Подключаем…');
	try {
		const result = await call('restore', {
			username: $('#username').value.trim(),
		});
		setConnected(result.username);
	} catch {
		showError($('#login-status'), tr('Не удалось открыть сохранённую сессию.'));
	} finally {
		button.disabled = false;
	}
});
$('#disconnect').addEventListener('click', async event => {
	const button = event.currentTarget;
	button.disabled = true;
	try {
		const cleared = await invoke('clear_instagram_login');
		if (!cleared?.nativeDisconnected) await call('disconnect');
		connected = false;
		syncing = false;
		clearTimeout(timer);
		posts = [];
		renderStamp = '';
		selectedThread = '';
		$('#threads').replaceChildren();
		$('#chat').replaceChildren(element('p', tr('Выберите переписку'), 'muted'));
		$('#days').replaceChildren();
		$('#profile-name').textContent = tr('Аккаунт не подключён.');
		$('#disconnect').hidden = true;
		$('#clear-cache').hidden = true;
		$('#login-form').reset();
		$('#login-form').hidden = true;
		$('#browser-step').hidden = false;
		$('#browser-complete').hidden = true;
		$('#browser-status').textContent = '';
		$('#login').showModal();
	} catch {
		showError($('#profile-name'));
	} finally {
		button.disabled = false;
	}
});
function dayKey(date) {
	return [
		date.getFullYear(),
		String(date.getMonth() + 1).padStart(2, '0'),
		String(date.getDate()).padStart(2, '0'),
	].join('-');
}
function reconcileChildren(parent, children) {
	for (let i = 0; i < children.length; i++) {
		if (parent.children[i] !== children[i])
			parent.insertBefore(children[i], parent.children[i] || null);
	}
	while (parent.children.length > children.length)
		parent.lastElementChild.remove();
}
function renderDays() {
	const stamp = JSON.stringify([
		dayKey(new Date()),
		syncing,
		posts.map(post => [
			post.id,
			post.thumbnail,
			post.username,
			post.publishedAt,
			post.caption,
			post.images,
		]),
	]);
	if (stamp === renderStamp) return;
	renderStamp = stamp;
	const groups = $('#days');
	const days = new Map([...groups.children].map(day => [day.dataset.day, day]));
	const tiles = new Map(
		[...groups.querySelectorAll('.tile')].map(tile => [
			tile.dataset.postId,
			tile,
		]),
	);
	const byDay = new Map();
	for (const post of posts) {
		const key = dayKey(new Date(post.publishedAt));
		if (!byDay.has(key)) byDay.set(key, []);
		byDay.get(key).push(post);
	}
	const sections = [];
	const now = new Date();
	for (let i = 0; i < 7; i++) {
		const date = new Date(
			now.getFullYear(),
			now.getMonth(),
			now.getDate() - i,
			12,
		);
		const key = dayKey(date);
		const section = days.get(key) || element('section', undefined, 'day');
		section.dataset.day = key;
		const title =
			i === 0
				? tr('Сегодня')
				: i === 1
					? tr('Вчера')
					: new Intl.DateTimeFormat(
							window.CalmI18n.getLanguage() === 'ru' ? 'ru-RU' : 'en-US',
							{
								day: 'numeric',
								month: 'long',
							},
						).format(date);
		const heading = section.querySelector('h2') || element('h2');
		heading.textContent = title;
		const entries = byDay.get(key) || [];
		if (!entries.length) {
			const message =
				section.querySelector('p.muted') || element('p', undefined, 'muted');
			message.textContent = syncing
				? tr('Ищем фотографии…')
				: tr('Фотографии не найдены');
			reconcileChildren(section, [heading, message]);
		} else {
			const authors = new Map();
			for (const post of entries) {
				const key = post.username.toLowerCase();
				if (!authors.has(key)) authors.set(key, []);
				authors.get(key).push(post);
			}
			const authorGrid =
				section.querySelector('.day-authors') ||
				element('div', undefined, 'day-authors');
			const existingAuthors = new Map(
				[...authorGrid.children].map(author => [
					author.dataset.username,
					author,
				]),
			);
			const authorNodes = [];
			for (const authorPosts of authors.values()) {
				const username = authorPosts[0].username.toLowerCase();
				const author =
					existingAuthors.get(username) ||
					element('section', undefined, 'day-author');
				author.dataset.username = username;
				author.style.setProperty(
					'--author-columns',
					Math.min(authorPosts.length, 3),
				);
				const authorHeading = author.querySelector('h3') || element('h3');
				authorHeading.textContent = '@' + authorPosts[0].username;
				const grid =
					author.querySelector('.grid') || element('div', undefined, 'grid');
				const photoNodes = [];
				for (const post of authorPosts) {
					let button = tiles.get(post.id);
					if (!button) {
						button = element('button', undefined, 'tile');
						button.type = 'button';
						button.dataset.postId = post.id;
						const image = element('img');
						image.alt = '';
						image.loading = 'lazy';
						image.referrerPolicy = 'no-referrer';
						button.append(image);
						button.addEventListener('click', () => openPhoto(button.post));
					}
					button.post = post;
					button.setAttribute('aria-label', tr('Фотография @') + post.username);
					const image = button.querySelector('img');
					const url = post.thumbnail || post.images[0];
					if (image.getAttribute('src') !== url) image.src = url;
					photoNodes.push(button);
				}
				reconcileChildren(grid, photoNodes);
				reconcileChildren(author, [authorHeading, grid]);
				authorNodes.push(author);
			}
			reconcileChildren(authorGrid, authorNodes);
			reconcileChildren(section, [heading, authorGrid]);
		}
		sections.push(section);
	}
	reconcileChildren(groups, sections);
}
function openPhoto(post) {
	currentPhoto = post;
	photoIndex = 0;
	$('#photo-author').textContent = '@' + post.username;
	$('#photo-caption').textContent = post.caption || '';
	$('#photo-caption').hidden = !post.caption;
	$('#photo-comments').replaceChildren();
	$('#photo-comment-input').value = '';
	$('#photo-comment-input').disabled = commentSending;
	$('#photo-comment-send').disabled = commentSending;
	$('#photo-comment-feedback').textContent = commentSending
		? tr('Отправляем предыдущий комментарий…')
		: '';
	$('#photo-comments-more').hidden = true;
	$('#photo-comments-status').textContent = tr(
		'Загружаем подпись и комментарии…',
	);
	showPhoto();
	$('#photo').showModal();
	void loadPhotoDetails(post, ++photoRequest);
}
async function loadPhotoDetails(post, request) {
	try {
		const details = await call('photo-details', {code: post.id});
		if (request !== photoRequest || currentPhoto !== post || !$('#photo').open)
			return;
		$('#photo-caption').textContent = details.caption || post.caption || '';
		$('#photo-caption').hidden = !$('#photo-caption').textContent;
		appendPhotoComments(details.comments);
		$('#photo-comments-more').hidden = !details.more;
		$('#photo-comments-status').textContent = details.comments.length
			? tr('Показаны комментарии из загруженной страницы.')
			: tr('На загруженной странице комментариев нет.');
	} catch {
		if (request === photoRequest && currentPhoto === post && $('#photo').open)
			$('#photo-comments-status').textContent = tr(
				'Не удалось загрузить комментарии. Попробуйте открыть фотографию ещё раз.',
			);
	}
}
function appendPhotoComments(comments) {
	const list = $('#photo-comments');
	for (const comment of comments) {
		const row = element('p', undefined, 'photo-comment');
		row.append(element('strong', '@' + comment.username + ' '));
		row.append(document.createTextNode(comment.text));
		list.append(row);
	}
}
$('#photo-comment-form').addEventListener('submit', async event => {
	event.preventDefault();
	if (commentSending || !currentPhoto) return;
	const input = $('#photo-comment-input');
	const text = input.value.trim();
	if (!text) return;
	const post = currentPhoto;
	const request = photoRequest;
	commentSending = true;
	input.disabled = true;
	$('#photo-comment-send').disabled = true;
	$('#photo-comment-feedback').textContent = tr('Отправляем…');
	try {
		const result = await call('photo-comment-send', {code: post.id, text});
		if (!result.sent) throw new Error('comment_unconfirmed');
		if (request === photoRequest && $('#photo').open) {
			input.value = '';
			appendPhotoComments([{username: result.username, text}]);
			$('#photo-comment-feedback').textContent = tr('Комментарий опубликован.');
		}
	} catch {
		if (request === photoRequest && $('#photo').open)
			$('#photo-comment-feedback').textContent = tr(
				'Отправка не подтверждена. Проверьте комментарии перед повторной отправкой.',
			);
	} finally {
		commentSending = false;
		input.disabled = false;
		$('#photo-comment-send').disabled = false;
		if (request !== photoRequest && $('#photo').open)
			$('#photo-comment-feedback').textContent = '';
	}
});
$('#photo-comments-more').addEventListener('click', async event => {
	const button = event.currentTarget;
	const request = photoRequest;
	const post = currentPhoto;
	button.disabled = true;
	try {
		const result = await call('photo-comments-more', {code: post.id});
		if (request !== photoRequest || !$('#photo').open) return;
		appendPhotoComments(result.comments);
		button.hidden = !result.more;
		$('#photo-comments-status').textContent = result.comments.length
			? ''
			: tr('На этой странице комментариев нет.');
	} catch {
		if (request === photoRequest && $('#photo').open)
			showError(
				$('#photo-comments-status'),
				tr('Не удалось загрузить комментарии.'),
			);
	} finally {
		button.disabled = false;
	}
});
async function openPhotoProfile() {
	const username = currentPhoto.username;
	const request = ++profileRequest;
	$('#photo').close();
	returnToAuthorProfile = false;
	$('#author-profile-name').textContent = '@' + username;
	$('#author-profile-fullname').textContent = '';
	$('#author-profile-bio').textContent = '';
	$('#author-profile-grid').replaceChildren();
	$('#author-profile-more').hidden = true;
	$('#author-profile-status').textContent = tr('Загружаем профиль…');
	$('#author-profile').showModal();
	try {
		const result = await call('profile-open', {username});
		if (request !== profileRequest || !$('#author-profile').open) return;
		$('#author-profile-fullname').textContent = result.name;
		$('#author-profile-bio').textContent = result.bio;
		appendProfilePhotos(result.photos);
		$('#author-profile-more').hidden = !result.more;
		$('#author-profile-status').textContent = result.photos.length
			? ''
			: tr('На этой странице фотографий нет.');
	} catch {
		if (request === profileRequest && $('#author-profile').open)
			showError(
				$('#author-profile-status'),
				tr('Не удалось загрузить профиль.'),
			);
	}
}
function appendProfilePhotos(photos) {
	const grid = $('#author-profile-grid');
	const existing = new Set(
		[...grid.children].map(child => child.dataset.postId),
	);
	for (const post of photos) {
		if (existing.has(post.id)) continue;
		existing.add(post.id);
		const button = element('button', undefined, 'tile');
		button.type = 'button';
		button.dataset.postId = post.id;
		button.setAttribute('aria-label', tr('Фотография @') + post.username);
		const image = element('img');
		image.src = post.thumbnail || post.images[0];
		image.alt = '';
		image.loading = 'lazy';
		image.referrerPolicy = 'no-referrer';
		button.append(image);
		button.addEventListener('click', () => {
			$('#author-profile').close();
			returnToAuthorProfile = true;
			openPhoto(post);
		});
		grid.append(button);
	}
}
$('#author-profile-more').addEventListener('click', async event => {
	const button = event.currentTarget;
	button.disabled = true;
	const request = profileRequest;
	try {
		const result = await call('profile-more');
		if (request !== profileRequest || !$('#author-profile').open) return;
		appendProfilePhotos(result.photos);
		button.hidden = !result.more;
		$('#author-profile-status').textContent = result.photos.length
			? ''
			: tr('На этой странице фотографий нет.');
	} catch {
		if (request === profileRequest && $('#author-profile').open)
			showError(
				$('#author-profile-status'),
				tr('Не удалось загрузить фотографии.'),
			);
	} finally {
		button.disabled = false;
	}
});
$('#close-author-profile').addEventListener('click', () => {
	profileRequest++;
	$('#author-profile').close();
});
$('#photo-author').addEventListener('click', openPhotoProfile);
$('#photo-profile').addEventListener('click', openPhotoProfile);
function showPhoto() {
	$('#photo-image').src = currentPhoto.images[photoIndex];
	$('#photo-image').alt = tr('Фотография @') + currentPhoto.username;
	$('#photo-count').textContent =
		photoIndex + 1 + ' / ' + currentPhoto.images.length;
	$('#prev').disabled = photoIndex === 0;
	$('#next').disabled = photoIndex === currentPhoto.images.length - 1;
}
$('#prev').addEventListener('click', () => {
	if (photoIndex > 0) {
		photoIndex--;
		showPhoto();
	}
});
$('#next').addEventListener('click', () => {
	if (photoIndex < currentPhoto.images.length - 1) {
		photoIndex++;
		showPhoto();
	}
});
$('#close-photo').addEventListener('click', () => {
	$('#photo').close();
	if (returnToAuthorProfile) {
		returnToAuthorProfile = false;
		$('#author-profile').showModal();
	}
});
$('#photo').addEventListener('cancel', event => {
	if (returnToAuthorProfile) {
		event.preventDefault();
		$('#close-photo').click();
	}
});
async function refreshCalendar() {
	if (!connected) return;
	if (syncing) {
		try {
			await call('calendar-stop');
			$('#calendar-status').textContent = tr('Останавливаем загрузку…');
		} catch {
			showError($('#calendar-status'));
		}
		return;
	}
	try {
		await call('calendar-start');
		syncing = true;
		$('#refresh').textContent = tr('Остановить');
		await pollCalendar();
	} catch {
		showError($('#calendar-status'));
	}
}
async function pollCalendar() {
	try {
		const state = await call('calendar-status', {revision: calendarRevision});
		if (state.unchanged) {
			if (syncing) timer = setTimeout(pollCalendar, 500);
			return;
		}
		calendarRevision = state.revision;
		posts = state.photos;
		syncing = state.status === 'loading';
		renderDays();
		const status = {
			loading:
				tr('Загружаем фотографии. Проверено подписок: ') + state.accounts + '.',
			complete:
				tr('Загрузка завершена. Проверено подписок: ') + state.accounts + '.',
			cancelled: tr('Загрузка остановлена.'),
			error:
				state.reason === 'feedback_required'
					? tr(
							'Instagram отклонил запрос приложения. Доступ может быть временно ограничен; попробуйте позже.',
						)
					: tr('Загрузка прервалась. Проверьте уведомления Instagram.'),
		}[state.status];
		$('#calendar-status').textContent =
			status +
			(state.incomplete ? tr(' Выборка может быть неполной.') : '') +
			(state.cached
				? tr(
						' Есть сохранённые ранее публикации; изображения загружаются из сети.',
					)
				: '');
		$('#refresh').textContent = syncing ? tr('Остановить') : tr('Обновить');
		if (syncing) timer = setTimeout(pollCalendar, 500);
	} catch {
		syncing = false;
		$('#refresh').textContent = tr('Обновить');
		showError($('#calendar-status'));
	}
}
$('#refresh').addEventListener('click', () => void refreshCalendar());
$('#clear-cache').addEventListener('click', async event => {
	const button = event.currentTarget;
	button.disabled = true;
	try {
		await call('calendar-clear');
		clearTimeout(timer);
		syncing = false;
		posts = [];
		renderStamp = '';
		renderDays();
		$('#refresh').textContent = tr('Обновить');
		$('#calendar-status').textContent = tr('Кеш удалён.');
	} catch {
		showError($('#calendar-status'), tr('Не удалось удалить кеш.'));
	} finally {
		button.disabled = false;
	}
});
async function loadThreads(more = false) {
	if (!connected) return;
	$('#refresh-messages').disabled = true;
	$('#message-status').textContent = tr('Загружаем переписки…');
	try {
		const result = await call('threads', {more});
		const list = $('#threads');
		if (!more) list.replaceChildren();
		else list.querySelector('.more-threads')?.remove();
		for (const thread of result.threads) {
			const button = element('button', thread.title, 'thread');
			button.type = 'button';
			button.addEventListener('click', () => void openThread(thread, button));
			list.append(button);
		}
		if (result.hasMore) {
			const next = element(
				'button',
				tr('Ещё переписки'),
				'thread more-threads',
			);
			next.type = 'button';
			next.addEventListener('click', () => {
				next.disabled = true;
				void loadThreads(true);
			});
			list.append(next);
		}
		$('#message-status').textContent = list.children.length
			? tr('Показаны загруженные переписки.')
			: tr('Переписки не найдены.');
	} catch {
		showError($('#message-status'));
		const next = $('#threads .more-threads');
		if (next) next.disabled = false;
	} finally {
		$('#refresh-messages').disabled = false;
	}
}
$('#refresh-messages').addEventListener('click', () => void loadThreads());
async function openThread(thread, button) {
	selectedThread = thread.id;
	document
		.querySelectorAll('.thread')
		.forEach(node => node.classList.toggle('active', node === button));
	const chat = $('#chat');
	chat.replaceChildren(
		element('h2', thread.title),
		element('p', tr('Загружаем…'), 'muted'),
	);
	try {
		const result = await call('messages', {thread: thread.id});
		if (selectedThread !== thread.id) return;
		chat.replaceChildren(element('h2', thread.title));
		const bubbles = element('div', undefined, 'bubbles');
		for (const message of result.messages)
			bubbles.append(messageBubble(message));
		let cursor = result.cursor;
		const history = element('button', tr('Ранние сообщения'), 'outline');
		history.type = 'button';
		history.hidden = !cursor;
		chat.append(history, bubbles);
		history.addEventListener('click', async () => {
			if (!cursor) return;
			history.disabled = true;
			try {
				const older = await call('messages', {thread: thread.id, cursor});
				if (selectedThread !== thread.id) return;
				const previousHeight = bubbles.scrollHeight;
				const fragment = document.createDocumentFragment();
				for (const message of older.messages)
					fragment.append(messageBubble(message));
				bubbles.prepend(fragment);
				bubbles.scrollTop += bubbles.scrollHeight - previousHeight;
				cursor = older.cursor;
				history.hidden = !cursor;
			} catch {
				showError(
					$('#message-status'),
					tr('Не удалось загрузить историю переписки.'),
				);
			} finally {
				history.disabled = false;
			}
		});
		const form = element('form', undefined, 'composer');
		const input = element('input');
		input.setAttribute('aria-label', tr('Сообщение'));
		input.maxLength = 1000;
		input.required = true;
		const send = element('button', tr('Отправить'), 'outline');
		send.type = 'submit';
		form.append(input, send);
		chat.append(form);
		const feedback = element('p', '', 'muted');
		feedback.setAttribute('role', 'status');
		chat.append(feedback);
		form.addEventListener('submit', async event => {
			event.preventDefault();
			const text = input.value.trim();
			if (!text) return;
			send.disabled = true;
			feedback.textContent = tr('Отправляем…');
			try {
				await call('send', {thread: thread.id, text});
				bubbles.append(element('p', text, 'bubble mine'));
				input.value = '';
				feedback.textContent = tr('Отправлено');
			} catch {
				feedback.textContent = tr(
					'Отправка не подтверждена. Проверьте диалог перед повторной отправкой.',
				);
			} finally {
				send.disabled = false;
			}
		});
		bubbles.scrollTop = bubbles.scrollHeight;
	} catch {
		showError($('#message-status'));
	}
}
function messageBubble(message) {
	const attachment = message.attachment || !message.text;
	const bubble = element(
		'p',
		attachment ? tr('Вложение') : message.text,
		'bubble' + (message.mine ? ' mine' : ''),
	);
	if (attachment) bubble.dataset.attachmentPlaceholder = 'true';
	return bubble;
}
$('#profile-name').textContent = tr('Аккаунт не подключён.');
window.CalmI18n.apply();
document
	.querySelectorAll('.language-select')
	.forEach(select =>
		select.addEventListener('change', () =>
			window.CalmI18n.setLanguage(select.value),
		),
	);
document.addEventListener('calmig-language-change', () => {
	if (!connected) $('#profile-name').textContent = tr('Аккаунт не подключён.');
	renderStamp = '';
	renderDays();
});
renderDays();
$('#login').showModal();
void (async () => {
	try {
		const result = await invoke('restore_instagram_login', {
			language: window.CalmI18n.getLanguage(),
		});
		if (result?.connected && !connected) setConnected(result.username);
	} catch {
		// An unavailable or expired session leaves the normal login screen usable.
	}
})();
