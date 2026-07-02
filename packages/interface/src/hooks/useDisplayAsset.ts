import {useEffect, useState} from 'react';

const HDR_QUERIES = ['(dynamic-range: high)', '(video-dynamic-range: high)'];

const readDisplayState = () => {
	if (typeof window === 'undefined') {
		return {oled: false, hdr: false};
	}

	const root = document.documentElement;
	return {
		oled:
			root.dataset.theme === 'oled' ||
			root.classList.contains('oled-theme'),
		hdr: HDR_QUERIES.some((query) => window.matchMedia(query).matches)
	};
};

export function useDisplayState() {
	const [state, setState] = useState(readDisplayState);

	useEffect(() => {
		if (typeof window === 'undefined') return;

		const update = () => setState(readDisplayState());
		const mediaQueries = HDR_QUERIES.map((query) =>
			window.matchMedia(query)
		);
		const observer = new MutationObserver(update);

		observer.observe(document.documentElement, {
			attributes: true,
			attributeFilter: ['class', 'data-theme']
		});

		mediaQueries.forEach((query) =>
			query.addEventListener('change', update)
		);
		update();

		return () => {
			observer.disconnect();
			mediaQueries.forEach((query) =>
				query.removeEventListener('change', update)
			);
		};
	}, []);

	return state;
}

export function useDisplayAsset<TAsset>({
	defaultAsset,
	oledAsset,
	oledHdrAsset
}: {
	defaultAsset: TAsset;
	oledAsset?: TAsset;
	oledHdrAsset?: TAsset;
}) {
	const {oled, hdr} = useDisplayState();

	if (oled && hdr && oledHdrAsset) return oledHdrAsset;
	if (oled && oledAsset) return oledAsset;
	return defaultAsset;
}
