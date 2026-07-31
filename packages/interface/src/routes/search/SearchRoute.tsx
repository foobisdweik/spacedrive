import {useEffect} from 'react';
import {useExplorer} from '../explorer';
import {ExplorerView} from '../explorer/ExplorerView';

export function SearchRoute() {
	const {enterSearchMode, exitSearchMode} = useExplorer();

	useEffect(() => {
		enterSearchMode('', 'library');

		return exitSearchMode;
	}, [enterSearchMode, exitSearchMode]);

	return <ExplorerView dedicatedSearch />;
}
