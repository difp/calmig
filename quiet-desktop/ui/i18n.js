'use strict';
(() => {
	const english = {
		'Основная навигация': 'Main navigation',
		'7 дней': '7 days',
		Сообщения: 'Messages',
		Профиль: 'Profile',
		'На основе instagram-cli': 'Based on instagram-cli',
		'Последние 7 дней': 'Last 7 days',
		Обновить: 'Refresh',
		'Только фотографии подписок, по дате публикации.':
			'Photos from followed accounts, ordered by date.',
		'Выберите переписку': 'Select a conversation',
		'Аккаунт не подключён.': 'No account connected.',
		'Приложение использует неофициальный доступ. Сессия сохраняется локально в файле.':
			'The app uses unofficial access. Your session is stored locally.',
		'Очистить кеш': 'Clear cache',
		'Выйти и удалить сессию': 'Sign out and remove session',
		Язык: 'Language',
		'Войти в Instagram': 'Sign in to Instagram',
		'Откройте официальную страницу, войдите в аккаунт и вернитесь в CalmIG. Приложение использует локальную сессию для доступа к фотографиям и перепискам.':
			'Open the official page, sign in, and return to CalmIG. The app uses a local session to access photos and messages.',
		'Открыть Instagram': 'Open Instagram',
		'Я вошёл': 'I have signed in',
		'Другой способ': 'Other sign-in method',
		'Вход выполняется через неофициальный клиент instagram-cli. Сессия сохраняется в локальном файле.':
			'Sign-in uses the unofficial instagram-cli client. Your session is stored locally.',
		'Имя пользователя': 'Username',
		Пароль: 'Password',
		Войти: 'Sign in',
		'Использовать сохранённую сессию': 'Use saved session',
		Назад: 'Back',
		Подтверждение: 'Verification',
		Код: 'Code',
		Продолжить: 'Continue',
		'Открыть профиль': 'Open profile',
		Закрыть: 'Close',
		'Предыдущая фотография': 'Previous photo',
		'Следующая фотография': 'Next photo',
		Комментарии: 'Comments',
		'Ваш комментарий': 'Your comment',
		Отправить: 'Send',
		'Ещё комментарии': 'More comments',
		'Показать ещё': 'Show more',
		'Не удалось выполнить запрос. Проверьте уведомления Instagram.':
			'Request failed. Check your Instagram notifications.',
		'Instagram ограничил частоту запросов. Попробуйте позже.':
			'Instagram has limited requests. Try again later.',
		'Не удалось связаться с Instagram. Проверьте сеть и настройки прокси.':
			'Could not reach Instagram. Check your network and proxy settings.',
		'Instagram просит подтвердить вход. Проверьте уведомления в официальном приложении.':
			'Instagram requires sign-in verification. Check the official app.',
		'Не удалось подготовить вход. Способ авторизации мог измениться.':
			'Could not prepare sign-in. The sign-in method may have changed.',
		'Instagram отклонил вход. Проверьте данные и уведомления.':
			'Instagram rejected sign-in. Check your details and notifications.',
		'Не удалось выполнить вход.': 'Could not sign in.',
		'Войдите в открывшемся окне, затем вернитесь сюда.':
			'Sign in in the opened window, then return here.',
		'Не удалось открыть страницу входа.': 'Could not open the sign-in page.',
		'Подключаем аккаунт…': 'Connecting account…',
		'Не удалось подключить сессию. Убедитесь, что вы вошли в Instagram.':
			'Could not connect the session. Make sure you are signed in to Instagram.',
		'Входим…': 'Signing in…',
		'Не удалось войти. Проверьте данные и уведомления Instagram.':
			'Could not sign in. Check your details and Instagram notifications.',
		'Проверяем…': 'Verifying…',
		'Нужен другой код подтверждения.':
			'A different verification code is required.',
		'Не удалось подтвердить вход. Проверьте уведомления Instagram.':
			'Could not verify sign-in. Check your Instagram notifications.',
		'Подключаем…': 'Connecting…',
		'Не удалось открыть сохранённую сессию.':
			'Could not open the saved session.',
		Сегодня: 'Today',
		Вчера: 'Yesterday',
		'Ищем фотографии…': 'Looking for photos…',
		'Фотографии не найдены': 'No photos found',
		'Фотография @': 'Photo by @',
		'Отправляем предыдущий комментарий…': 'Sending your previous comment…',
		'Загружаем подпись и комментарии…': 'Loading caption and comments…',
		'Показаны комментарии из загруженной страницы.':
			'Showing comments from the loaded page.',
		'На загруженной странице комментариев нет.':
			'There are no comments on the loaded page.',
		'Не удалось загрузить комментарии. Попробуйте открыть фотографию ещё раз.':
			'Could not load comments. Try reopening the photo.',
		'Отправляем…': 'Sending…',
		'Комментарий опубликован.': 'Comment published.',
		'Отправка не подтверждена. Проверьте комментарии перед повторной отправкой.':
			'Sending was not confirmed. Check the comments before trying again.',
		'На этой странице комментариев нет.': 'There are no comments on this page.',
		'Не удалось загрузить комментарии.': 'Could not load comments.',
		'Загружаем профиль…': 'Loading profile…',
		'На этой странице фотографий нет.': 'There are no photos on this page.',
		'Не удалось загрузить профиль.': 'Could not load the profile.',
		'Не удалось загрузить фотографии.': 'Could not load photos.',
		'Останавливаем загрузку…': 'Stopping…',
		Остановить: 'Stop',
		'Загружаем фотографии. Проверено подписок: ':
			'Loading photos. Accounts checked: ',
		'Загрузка завершена. Проверено подписок: ':
			'Loading complete. Accounts checked: ',
		'Загрузка остановлена.': 'Loading stopped.',
		'Загрузка прервалась. Проверьте уведомления Instagram.':
			'Loading stopped unexpectedly. Check Instagram notifications.',
		' Выборка может быть неполной.': ' Results may be incomplete.',
		' Есть сохранённые ранее публикации; изображения загружаются из сети.':
			' Previously saved posts are shown; images load from the network.',
		'Кеш удалён.': 'Cache cleared.',
		'Не удалось удалить кеш.': 'Could not clear the cache.',
		'Загружаем переписки…': 'Loading conversations…',
		'Ещё переписки': 'More conversations',
		'Показаны загруженные переписки.': 'Showing loaded conversations.',
		'Переписки не найдены.': 'No conversations found.',
		'Загружаем…': 'Loading…',
		Вложение: 'Attachment',
		'Ранние сообщения': 'Earlier messages',
		'Не удалось загрузить историю переписки.':
			'Could not load conversation history.',
		Сообщение: 'Message',
		Отправлено: 'Sent',
		'Отправка не подтверждена. Проверьте диалог перед повторной отправкой.':
			'Sending was not confirmed. Check the conversation before trying again.',
	};
	let language = 'ru';
	try {
		if (localStorage.getItem('calmig-language') === 'en') language = 'en';
	} catch {}
	const reverse = new Map(Object.entries(english).map(([ru, en]) => [en, ru]));
	const dynamicParts = [
		'Загружаем фотографии. Проверено подписок: ',
		'Загрузка завершена. Проверено подписок: ',
		' Выборка может быть неполной.',
		' Есть сохранённые ранее публикации; изображения загружаются из сети.',
		'Фотография @',
	];
	function t(value) {
		return language === 'en' ? (english[value] ?? value) : value;
	}
	function translate(value) {
		const normalized = value.trim().replace(/\s+/g, ' ');
		const ru = english[normalized] ? normalized : reverse.get(normalized);
		if (ru) return t(ru);
		let output = value;
		for (const key of dynamicParts)
			output = output.replaceAll(
				language === 'en' ? key : english[key],
				language === 'en' ? english[key] : key,
			);
		return output;
	}
	function apply() {
		document.documentElement.lang = language;
		const walker = document.createTreeWalker(
			document.body,
			NodeFilter.SHOW_TEXT,
		);
		const nodes = [];
		while (walker.nextNode()) nodes.push(walker.currentNode);
		for (const node of nodes) {
			const parent = node.parentElement;
			if (
				!parent ||
				parent.closest(
					'#photo-comments, #photo-caption, .bubbles, .thread:not(.more-threads), #author-profile-name, #author-profile-fullname, #author-profile-bio, #profile-name, #chat h2, textarea, script, style',
				)
			)
				continue;
			node.nodeValue = translate(node.nodeValue);
		}
		for (const node of document.querySelectorAll(
			'[aria-label], [title], img[alt]',
		)) {
			for (const name of ['aria-label', 'title', 'alt'])
				if (node.hasAttribute(name))
					node.setAttribute(name, translate(node.getAttribute(name)));
		}
		for (const select of document.querySelectorAll('.language-select'))
			select.value = language;
		for (const node of document.querySelectorAll(
			'[data-attachment-placeholder]',
		))
			node.textContent = t('Вложение');
	}
	function setLanguage(value) {
		language = value === 'en' ? 'en' : 'ru';
		try {
			localStorage.setItem('calmig-language', language);
		} catch {}
		apply();
		document.dispatchEvent(new Event('calmig-language-change'));
	}
	window.CalmI18n = {t, apply, setLanguage, getLanguage: () => language};
})();
