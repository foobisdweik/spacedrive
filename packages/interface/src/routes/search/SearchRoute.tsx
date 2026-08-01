import {useCallback, useEffect, useRef} from 'react';
import {useSearchParams} from 'react-router-dom';
import {useExplorer} from '../explorer';
import {ExplorerView} from '../explorer/ExplorerView';

export function SearchRoute() {
	const {enterSearchMode, exitSearchMode} = useExplorer();
	const [searchParams, setSearchParams] = useSearchParams();
	const initialSearchQuery = useRef(searchParams.get('q') ?? '').current;

	useEffect(() => {
		enterSearchMode(initialSearchQuery, 'library');

		return exitSearchMode;
	}, [enterSearchMode, exitSearchMode, initialSearchQuery]);

	const handleSearchQueryChange = useCallback(
		(query: string) => {
			setSearchParams(
				(currentParams) => {
					const nextParams = new URLSearchParams(currentParams);
					if (query) {
						nextParams.set('q', query);
					} else {
						nextParams.delete('q');
					}
					return nextParams;
				},
				{replace: true}
			);
		},
		[setSearchParams]
	);

	return (
		<ExplorerView
			dedicatedSearch
			initialSearchQuery={initialSearchQuery}
			onSearchQueryChange={handleSearchQueryChange}
		/>
	);
}
