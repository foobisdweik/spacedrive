import {sounds} from '@sd/assets/sounds';
import {
	useJobs as useJobsCore,
	type ExtendedJobListItem,
	type UseJobsReturn
} from '@sd/ts-client';
import {useQueryClient} from '@tanstack/react-query';
import {useEffect, useRef} from 'react';
import {useVolumeIndexingStore} from '../../../stores/volumeIndexingStore';

// Global set to track which jobs have already played their completion sound
// This prevents multiple hook instances from playing the sound multiple times
const completedJobSounds = new Set<string>();

// Global throttle to prevent multiple sounds within 5 seconds
let lastSoundPlayedAt = 0;
const SOUND_THROTTLE_MS = 5000;
const FILE_QUERY_JOB_NAMES = new Set(['filecopy', 'movefiles', 'deletefiles', 'indexer']);

/**
 * Desktop-specific wrapper around useJobs that adds:
 * - Sound system for job completion
 * - Volume indexing tracking integration
 */
export function useJobs(): UseJobsReturn {
	const {setVolumeJob, clearVolumeJob} = useVolumeIndexingStore();
	const queryClient = useQueryClient();
	const jobsRef = useRef<ExtendedJobListItem[]>([]);

	const invalidateFileQueries = () => {
		queryClient.invalidateQueries({
			predicate: (query) =>
				Array.isArray(query.queryKey) &&
				typeof query.queryKey[0] === 'string' &&
				(query.queryKey[0] === 'query:files.directory_listing' ||
					query.queryKey[0] === 'query:search.files' ||
					query.queryKey[0] === 'query:locations.list' ||
					query.queryKey[0] === 'query:files.by_id')
		});
	};

	const shouldInvalidateFileQueries = (jobId: string, jobType?: string) => {
		const job = jobsRef.current.find((currentJob) => currentJob.id === jobId);
		const identifiers = [jobType, job?.name].filter(
			(identifier): identifier is string => !!identifier
		);

		return identifiers.some((identifier) =>
			FILE_QUERY_JOB_NAMES.has(identifier.toLowerCase().replace(/[^a-z0-9]/g, ''))
		);
	};

	const jobs = useJobsCore({
		onJobCompleted: (jobId, jobType) => {
			if (shouldInvalidateFileQueries(jobId, jobType)) {
				invalidateFileQueries();
			}

			// Play completion sound
			if (!completedJobSounds.has(jobId)) {
				completedJobSounds.add(jobId);

				// Throttle: only play sound if enough time has passed since last sound
				const now = Date.now();
				if (now - lastSoundPlayedAt >= SOUND_THROTTLE_MS) {
					lastSoundPlayedAt = now;

					// Play job-specific sound
					if (
						jobType?.includes('copy') ||
						jobType?.includes('Copy')
					) {
						sounds.copy();
					} else {
						sounds.jobDone();
					}
				}

				// Clean up old entries after 5 seconds to prevent memory leak
				setTimeout(() => completedJobSounds.delete(jobId), 5000);
			}
		},
		onJobFailed: (jobId) => {
			if (shouldInvalidateFileQueries(jobId)) {
				invalidateFileQueries();
			}
		},
		onJobCancelled: (jobId) => {
			if (shouldInvalidateFileQueries(jobId)) {
				invalidateFileQueries();
			}
		}
	});

	// Ref for stable jobs access
	useEffect(() => {
		jobsRef.current = jobs.jobs;
	}, [jobs.jobs]);

	// Track volume indexing jobs
	useEffect(() => {
		jobs.jobs.forEach((job) => {
			if (job.name === 'indexer' && job.status === 'running') {
				const context = job.action_context?.context as any;
				const volumeFingerprint = context?.volume_fingerprint;
				if (
					volumeFingerprint &&
					typeof volumeFingerprint === 'string'
				) {
					setVolumeJob(volumeFingerprint, job.id);
				}
			}
		});
	}, [jobs.jobs, setVolumeJob]);

	// Clear volume jobs on completion/failure/cancellation
	useEffect(() => {
		// This effect watches for jobs that are no longer in the list or are no longer running
		const runningIndexerJobs = jobs.jobs.filter(
			(j) => j.name === 'indexer' && j.status === 'running'
		);

		// Find any volume jobs that need to be cleared
		jobsRef.current.forEach((prevJob) => {
			if (
				prevJob.name === 'indexer' &&
				!runningIndexerJobs.find((j) => j.id === prevJob.id)
			) {
				const context = prevJob.action_context?.context as any;
				const volumeFingerprint = context?.volume_fingerprint;
				if (
					volumeFingerprint &&
					typeof volumeFingerprint === 'string'
				) {
					clearVolumeJob(volumeFingerprint);
				}
			}
		});
	}, [jobs.jobs, clearVolumeJob]);

	return jobs;
}
