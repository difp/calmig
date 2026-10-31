import type {LoginResult} from '../client.js';

export type LoginFailure =
	| 'rate_limited'
	| 'network'
	| 'checkpoint'
	| 'unavailable'
	| 'rejected';

/** Only fixed categories cross into the UI; upstream response text can contain secrets. */
export function loginFailure(result: LoginResult): LoginFailure {
	if (result.checkpointError) return 'checkpoint';
	const message = result.error ?? '';
	if (/\b429\b|too many requests/i.test(message)) return 'rate_limited';
	if (/\/api\/v1\/qe\/sync\/.*\b401\b/i.test(message)) return 'unavailable';
	if (/checkpoint|challenge_required/i.test(message)) return 'checkpoint';
	if (
		/network|proxy|timed? ?out|econn|enotfound|eai_again|http2|stream.*cancel|tls|certificate/i.test(
			message,
		)
	)
		return 'network';
	if (/login context|login flow may have changed/i.test(message))
		return 'unavailable';
	return 'rejected';
}
