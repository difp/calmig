import type {IgApiClient} from 'instagram-private-api';

/** The upstream library defaults to strictSSL: false. Never disable certificate verification. */
export function enforceTls(client: IgApiClient): void {
	const {request} = client;
	const send = request.send.bind(request);
	request.send = async (options, onlyCheckHttpStatus) =>
		send({...options, strictSSL: true}, onlyCheckHttpStatus);
}
