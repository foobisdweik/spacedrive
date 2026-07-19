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
	ready: boolean,
	horizontalScrollRef?: RefObject<HTMLElement | null>
) {
	// Some views (e.g. ListView) scroll vertically and horizontally on two
	// different elements (a sticky-header layout keeps horizontal scroll on
	// an inner wrapper). Default to the same ref when the caller only has one
	// scroll container.
	const hScrollRef = horizontalScrollRef ?? scrollRef;

	const {activeTabId, scrollPosition, setScrollPosition} = useExplorer();
	const restoredTabRef = useRef<string | null>(null);
	const saveTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);
	const activeTabIdRef = useRef(activeTabId);

	// Keep activeTabIdRef up to date
	useLayoutEffect(() => {
		activeTabIdRef.current = activeTabId;
	}, [activeTabId]);

	// Restore the saved offset for the current tab (once per tab).
	useLayoutEffect(() => {
		const el = scrollRef.current;
		const hEl = hScrollRef.current;
		if (!el || !hEl || !ready) return;
		if (restoredTabRef.current === activeTabId) return;
		el.scrollTop = scrollPosition.top;
		hEl.scrollLeft = scrollPosition.left;
		restoredTabRef.current = activeTabId;
	}, [
		activeTabId,
		ready,
		scrollPosition.top,
		scrollPosition.left,
		scrollRef,
		hScrollRef
	]);

	useEffect(
		() => () => {
			if (saveTimeout.current) clearTimeout(saveTimeout.current);
		},
		[]
	);

	return (_event: UIEvent<HTMLElement>) => {
		// Ignore scroll events that fire before this tab's position has been
		// restored, so the programmatic restore isn't clobbered by a transient 0.
		if (restoredTabRef.current !== activeTabId) return;
		// Read directly from both scroll elements rather than event.currentTarget,
		// since this handler may be attached to either (or both) of them when
		// vertical and horizontal scroll live on different elements.
		const scrollTop = scrollRef.current?.scrollTop ?? 0;
		const scrollLeft = hScrollRef.current?.scrollLeft ?? 0;
		const currentTabId = activeTabId;
		if (saveTimeout.current) clearTimeout(saveTimeout.current);
		saveTimeout.current = setTimeout(() => {
			if (activeTabIdRef.current === currentTabId) {
				setScrollPosition({top: scrollTop, left: scrollLeft});
			}
		}, 150);
	};
}
