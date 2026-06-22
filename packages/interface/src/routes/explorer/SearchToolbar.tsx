import {FunnelSimple, X} from '@phosphor-icons/react';
import clsx from 'clsx';
import {useExplorer} from './context';
import type {SearchScope} from './context';

export function SearchToolbar() {
	const explorer = useExplorer();

	if (explorer.mode.type !== 'search' && explorer.mode.type !== 'filtered') {
		return null;
	}

	if (explorer.mode.type === 'filtered') {
		return (
			<div className="border-sidebar-line/30 bg-sidebar-box/10 flex items-center gap-3 border-b px-4 py-2">
				<div className="flex items-center gap-2">
					<FunnelSimple
						className="text-sidebar-inkDull size-3.5"
						weight="bold"
					/>
					<span className="text-sidebar-inkDull text-xs font-medium">
						Filtered by:
					</span>
					<span className="text-sidebar-ink text-xs font-semibold">
						{explorer.mode.label}
					</span>
				</div>

				<div className="flex-1" />

				<button
					onClick={explorer.exitFilteredMode}
					className={clsx(
						'flex items-center gap-1.5 rounded-md px-2 py-1',
						'text-sidebar-inkDull text-xs font-medium',
						'hover:bg-sidebar-selected/40 hover:text-sidebar-ink transition-colors'
					)}
				>
					<X className="size-3.5" weight="bold" />
					Clear Filter
				</button>
			</div>
		);
	}

	const {scope} = explorer.mode;

	const handleScopeChange = (newScope: SearchScope) => {
		if (explorer.mode.type === 'search') {
			explorer.enterSearchMode(explorer.mode.query, newScope);
		}
	};

	return (
		<div className="border-sidebar-line/30 bg-sidebar-box/10 flex items-center gap-3 border-b px-4 py-2">
			<div className="flex items-center gap-2">
				<span className="text-sidebar-inkDull text-xs font-medium">
					Search in:
				</span>
				<div className="bg-sidebar-box/30 flex items-center gap-1 rounded-lg p-0.5">
					<ScopeButton
						active={scope === 'folder'}
						onClick={() => handleScopeChange('folder')}
					>
						This Folder
					</ScopeButton>
					<ScopeButton
						active={scope === 'location'}
						onClick={() => handleScopeChange('location')}
					>
						Location
					</ScopeButton>
					<ScopeButton
						active={scope === 'library'}
						onClick={() => handleScopeChange('library')}
					>
						Library
					</ScopeButton>
				</div>
			</div>

			<div className="bg-sidebar-line/30 h-4 w-px" />

			<button
				className={clsx(
					'flex items-center gap-1.5 rounded-md px-2 py-1',
					'text-sidebar-ink text-xs font-medium',
					'hover:bg-sidebar-selected/40 transition-colors'
				)}
			>
				<FunnelSimple className="size-3.5" weight="bold" />
				Filters
			</button>

			<div className="flex-1" />

			<button
				onClick={explorer.exitSearchMode}
				className={clsx(
					'flex items-center gap-1.5 rounded-md px-2 py-1',
					'text-sidebar-inkDull text-xs font-medium',
					'hover:bg-sidebar-selected/40 hover:text-sidebar-ink transition-colors'
				)}
			>
				<X className="size-3.5" weight="bold" />
				Clear Search
			</button>
		</div>
	);
}

interface ScopeButtonProps {
	active: boolean;
	onClick: () => void;
	children: React.ReactNode;
}

function ScopeButton({active, onClick, children}: ScopeButtonProps) {
	return (
		<button
			onClick={onClick}
			className={clsx(
				'rounded-md px-3 py-1 text-xs font-medium transition-all',
				active
					? 'bg-accent text-white shadow-sm'
					: 'text-sidebar-inkDull hover:text-sidebar-ink hover:bg-sidebar-selected/30'
			)}
		>
			{children}
		</button>
	);
}
