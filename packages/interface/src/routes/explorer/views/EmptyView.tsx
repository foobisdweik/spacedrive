import {Folder} from '@phosphor-icons/react';

interface EmptyViewProps {
	/** Primary message. Defaults to the "no location selected" prompt. */
	title?: string;
	/** Optional secondary line. */
	subtitle?: string;
	/** Show a folder illustration (used for empty directories). */
	showIcon?: boolean;
}

export function EmptyView({
	title = 'Select a location from the sidebar to browse files',
	subtitle,
	showIcon = false
}: EmptyViewProps = {}) {
	return (
		<div className="flex items-center justify-center h-full">
			<div className="flex flex-col items-center gap-2 text-center">
				{showIcon && (
					<Folder className="size-10 text-ink-faint" weight="thin" />
				)}
				<div className="text-ink-dull text-sm">{title}</div>
				{subtitle && (
					<div className="text-ink-faint text-xs">{subtitle}</div>
				)}
			</div>
		</div>
	);
}
