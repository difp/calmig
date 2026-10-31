/* eslint-disable @typescript-eslint/no-unsafe-call */
import test from 'ava';
import {loginFailure} from '../source/quiet/login-errors.js';

test('login errors expose only safe fixed categories', t => {
	t.is(
		loginFailure({
			success: false,
			error: 'POST /api/v1/qe/sync/ - 401 Unauthorized',
		}),
		'unavailable',
	);
	t.is(
		loginFailure({success: false, error: 'HTTP 429 SECRET_COOKIE'}),
		'rate_limited',
	);
	t.is(
		loginFailure({success: false, error: 'stream canceled SECRET_COOKIE'}),
		'network',
	);
	t.is(
		loginFailure({
			success: false,
			error: 'Instagram login flow may have changed SECRET_COOKIE',
		}),
		'unavailable',
	);
	t.is(
		loginFailure({success: false, error: 'unexpected SECRET_COOKIE'}),
		'rejected',
	);
});
