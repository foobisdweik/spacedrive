import {
	DiscordLogo,
	GithubLogo,
	GlobeHemisphereWest
} from '@phosphor-icons/react';
import {BallBlue, BallBlue_OLED, BallBlue_OLED_HDR} from '@sd/assets/images';
import {CircleButton} from '@spacedrive/primitives';
import {motion} from 'framer-motion';
import Orb from '../../components/Orb';
import contributors from '../../contributors.json';
import {useDisplayAsset} from '../../hooks/useDisplayAsset';

export function AboutSettings() {
	const ballBlue = useDisplayAsset({
		defaultAsset: BallBlue,
		oledAsset: BallBlue_OLED,
		oledHdrAsset: BallBlue_OLED_HDR
	});

	return (
		<div className="flex min-h-[600px] flex-col items-center justify-center">
			{/* Animated orb with ball */}
			<motion.div
				initial={{scale: 0.8, opacity: 0}}
				animate={{scale: 1, opacity: 1}}
				transition={{duration: 0.6, ease: 'easeOut'}}
				className="relative mb-8 h-64 w-64"
			>
				{/* Ball image - behind the orb */}
				<div className="absolute inset-[8%] z-0">
					<img
						src={ballBlue}
						alt="Spacedrive"
						className="h-full w-full select-none object-contain"
						draggable={false}
					/>
				</div>
				{/* Orb animation - inset to make it smaller */}
				<div className="absolute inset-[15%] z-10">
					<Orb
						palette="blue"
						hue={0}
						hoverIntensity={0}
						rotateOnHover={false}
						forceHoverState={true}
					/>
				</div>
			</motion.div>

			{/* Branding */}
			<motion.div
				initial={{opacity: 0, y: 10}}
				animate={{opacity: 1, y: 0}}
				transition={{duration: 0.5, delay: 0.3}}
				className="mb-6 text-center"
			>
				<h3 className="mb-2 text-2xl font-bold text-white">
					Spacedrive
				</h3>
				<p className="text-sm text-white/60">
					A file explorer from the future.
				</p>
			</motion.div>

			{/* Manifesto */}
			<motion.div
				initial={{opacity: 0, y: 10}}
				animate={{opacity: 1, y: 0}}
				transition={{duration: 0.5, delay: 0.35}}
				className="mb-8 max-w-md px-4 text-center"
			>
				<p className="text-sm leading-relaxed text-white/70">
					Infrastructure for the next era of computing. An
					architecture designed for multi-device environments from the
					ground up—not cloud services retrofitted with offline
					support, but local-first sync that scales to the cloud when
					you want it.
				</p>
			</motion.div>

			{/* Links */}
			<motion.div
				initial={{opacity: 0, y: 10}}
				animate={{opacity: 1, y: 0}}
				transition={{duration: 0.5, delay: 0.4}}
				className="mb-6 flex gap-3"
			>
				<a
					href="https://spacedrive.com"
					target="_blank"
					rel="noopener noreferrer"
				>
					<CircleButton icon={GlobeHemisphereWest}>
						Website
					</CircleButton>
				</a>
				<a
					href="https://github.com/spacedriveapp/spacedrive"
					target="_blank"
					rel="noopener noreferrer"
				>
					<CircleButton icon={GithubLogo}>GitHub</CircleButton>
				</a>
				<a
					href="https://discord.gg/spacedrive"
					target="_blank"
					rel="noopener noreferrer"
				>
					<CircleButton icon={DiscordLogo}>Discord</CircleButton>
				</a>
			</motion.div>

			{/* Contributors */}
			<motion.div
				initial={{opacity: 0, y: 10}}
				animate={{opacity: 1, y: 0}}
				transition={{duration: 0.5, delay: 0.45}}
				className="mb-8 max-w-lg px-4 text-center"
			>
				<p className="text-[11px] leading-relaxed text-white/30">
					{contributors.map(
						(c: {name: string; github: string}, i) => (
							<span key={c.github}>
								{i > 0 && ' · '}
								<a
									href={`https://github.com/${c.github}`}
									target="_blank"
									rel="noopener noreferrer"
									title={`@${c.github}`}
									className="transition-colors hover:text-white/50"
								>
									{c.name}
								</a>
							</span>
						)
					)}
				</p>
			</motion.div>

			{/* License */}
			<motion.div
				initial={{opacity: 0}}
				animate={{opacity: 1}}
				transition={{duration: 0.5, delay: 0.55}}
				className="text-center"
			>
				<a
					href="https://github.com/spacedriveapp/spacedrive/blob/main/LICENSE"
					target="_blank"
					rel="noopener noreferrer"
					className="text-sm text-white/40 transition-colors hover:text-white/60"
				>
					FSL-1.1-ALv2
				</a>
			</motion.div>
		</div>
	);
}
