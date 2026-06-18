import { ArrowsClockwise, CircleNotch, Warning } from "@phosphor-icons/react";

import { ActivityFeed } from "./components/ActivityFeed";
import { PeerList } from "./components/PeerList";
import { useSyncMonitor } from "./hooks/useSyncMonitor";

export function SyncMonitorRoute() {
	const sync = useSyncMonitor();
	const onlinePeerCount = sync.peers.filter((peer) => peer.isOnline).length;

	const statusColor = (() => {
		switch (sync.currentState) {
			case "Ready":
				return "bg-green-500";
			case "Backfilling":
				return "bg-yellow-500";
			case "CatchingUp":
				return "bg-accent";
			case "Paused":
				return "bg-ink-dull";
			default:
				return "bg-ink-faint";
		}
	})();

	return (
		<div className="h-full overflow-auto p-6">
			<div className="mx-auto flex max-w-5xl flex-col gap-6">
				<div className="flex items-start justify-between gap-4">
					<div>
						<h1 className="text-ink text-2xl font-bold">
							Sync Monitor
						</h1>
						<div className="mt-2 flex items-center gap-2 text-sm text-ink-dull">
							<div className={`size-2 rounded-full ${statusColor}`} />
							<span>{sync.currentState}</span>
						</div>
					</div>

					<div className="grid grid-cols-3 gap-2 text-right">
						<div className="rounded-lg border border-app-line bg-app-box px-3 py-2">
							<div className="text-lg font-semibold text-ink">
								{onlinePeerCount}
							</div>
							<div className="text-xs text-ink-faint">
								Online
							</div>
						</div>
						<div className="rounded-lg border border-app-line bg-app-box px-3 py-2">
							<div className="text-lg font-semibold text-ink">
								{sync.peers.length}
							</div>
							<div className="text-xs text-ink-faint">
								Peers
							</div>
						</div>
						<div className="rounded-lg border border-app-line bg-app-box px-3 py-2">
							<div className="text-lg font-semibold text-ink">
								{sync.errorCount}
							</div>
							<div className="text-xs text-ink-faint">
								Errors
							</div>
						</div>
					</div>
				</div>

				<div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,420px)]">
					<section className="min-h-[360px] rounded-lg border border-app-line bg-app/60">
						<div className="flex items-center gap-2 border-b border-app-line px-4 py-3">
							{sync.currentState === "Backfilling" ||
							sync.currentState === "CatchingUp" ? (
								<CircleNotch
									className="size-4 animate-spin text-accent"
									weight="bold"
								/>
							) : (
								<ArrowsClockwise
									className="size-4 text-ink-dull"
									weight="bold"
								/>
							)}
							<h2 className="text-sm font-semibold text-ink">
								Peers
							</h2>
						</div>
						<PeerList
							peers={sync.peers}
							currentState={sync.currentState}
						/>
					</section>

					<section className="min-h-[360px] rounded-lg border border-app-line bg-app/60">
						<div className="flex items-center justify-between border-b border-app-line px-4 py-3">
							<h2 className="text-sm font-semibold text-ink">
								Activity
							</h2>
							{sync.errorCount > 0 && (
								<div className="flex items-center gap-1 text-xs text-red-400">
									<Warning className="size-4" weight="bold" />
									<span>{sync.errorCount}</span>
								</div>
							)}
						</div>
						<ActivityFeed activities={sync.recentActivity} />
					</section>
				</div>
			</div>
		</div>
	);
}
