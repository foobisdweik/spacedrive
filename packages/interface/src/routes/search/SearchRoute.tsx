import {useCallback, useEffect} from 'react';
import {useSearchParams} from 'react-router-dom';
import {useExplorer} from '../explorer';
import {ExplorerView} from '../explorer/ExplorerView';

export function SearchRoute() {
	const {exitSearchMode} = useExplorer();
	const [searchParams, setSearchParams] = useSearchParams();
	const searchQuery = searchParams.get('q') ?? '';

	useEffect(() => {
		return exitSearchMode;
	}, [exitSearchMode]);

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
			searchQuery={searchQuery}
			onSearchQueryChange={handleSearchQueryChange}
		/>
	);
}
