import {ArrowLeft, House, Warning} from '@phosphor-icons/react';
import {Button} from '@spacedrive/primitives';
import {useExplorer} from '../context';

interface MissingPathViewProps {
	error: Error;
}

export function MissingPathView({error}: MissingPathViewProps) {
	const {goBack, canGoBack, navigateToView} = useExplorer();

	const handleGoHome = () => {
		navigateToView('overview');
	};

	return (
		<div className="mx-auto flex h-full w-full max-w-md flex-col items-center justify-center px-6 py-12 text-center">
			<div className="mb-6 rounded-full bg-red-500/10 p-4 text-red-500">
				<Warning size={48} weight="duotone" />
			</div>
			<h3 className="text-ink mb-2 text-lg font-semibold">
				Path not found
			</h3>
			<p className="text-ink-dull mb-8 text-sm leading-relaxed">
				The selected directory does not exist, has been moved, or the
				storage device is disconnected.
			</p>

			<div className="flex w-full items-center gap-3">
				<Button
					onClick={goBack}
					disabled={!canGoBack}
					className="flex-1 justify-center gap-2"
					variant="outline"
				>
					<ArrowLeft size={16} />
					Go Back
				</Button>
				<Button
					onClick={handleGoHome}
					className="flex-1 justify-center gap-2"
					variant="accent"
				>
					<House size={16} />
					Go to Overview
				</Button>
			</div>
			{error.message && (
				<details className="border-border/40 bg-app-light/30 mt-8 w-full rounded-lg border p-3 text-left">
					<summary className="text-ink-dull cursor-pointer select-none text-xs font-medium">
						Error details
					</summary>
					<pre className="mt-2 max-h-32 overflow-auto whitespace-pre-wrap font-mono text-[10px] leading-normal text-red-400">
						{error.message}
					</pre>
				</details>
			)}
		</div>
	);
}
