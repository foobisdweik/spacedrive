import {useEffect, useLayoutEffect, useRef} from 'react';
import type {RefObject, UIEvent} from 'react';
import {useExplorer} from '../context';

/**
 * Preserve a scroll container's offset per Explorer tab.
 *
 * Views mount a fresh virtualizer/scroll element on every tab or view switch,
 * which resets the scroll offset to the top. This hook restores the tab's saved
 * offset once the content is measured and populated (`ready`), re-applying it
 * whenever the active tab changes, and persists new offsets (throttled) as the
 * user scrolls.
 *
 * @param scrollRef ref to the scrollable element
 * @param ready true once the element is laid out and has content to scroll
 * @returns an onScroll handler to attach to the scrollable element
 */
export function useScrollRestoration(
	scrollRef: RefObject<HTMLElement | null>,
	ready: boolean
) {
	const {activeTabId, scrollPosition, setScrollPosition} = useExplorer();
	const restoredTabRef = useRef<string | null>(null);
	const saveTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);

	// Restore the saved offset for the current tab (once per tab).
	useLayoutEffect(() => {
		const el = scrollRef.current;
		if (!el || !ready) return;
		if (restoredTabRef.current === activeTabId) return;
		el.scrollTop = scrollPosition.top;
		el.scrollLeft = scrollPosition.left;
		restoredTabRef.current = activeTabId;
	}, [
		activeTabId,
		ready,
		scrollPosition.top,
		scrollPosition.left,
		scrollRef
	]);

	useEffect(
		() => () => {
			if (saveTimeout.current) clearTimeout(saveTimeout.current);
		},
		[]
	);

	return (event: UIEvent<HTMLElement>) => {
		// Ignore scroll events that fire before this tab's position has been
		// restored, so the programmatic restore isn't clobbered by a transient 0.
		if (restoredTabRef.current !== activeTabId) return;
		const {scrollTop, scrollLeft} = event.currentTarget;
		if (saveTimeout.current) clearTimeout(saveTimeout.current);
		saveTimeout.current = setTimeout(() => {
			setScrollPosition({top: scrollTop, left: scrollLeft});
		}, 150);
	};
}
