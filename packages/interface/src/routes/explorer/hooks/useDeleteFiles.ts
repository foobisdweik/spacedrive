import type {File} from '@sd/ts-client';
import {useCallback, useRef} from 'react';
import {usePlatform} from '../../../contexts/PlatformContext';
import {useLibraryMutation} from '../../../contexts/SpacedriveContext';

function platformConfirm(
	platform: ReturnType<typeof usePlatform>,
	message: string
): Promise<boolean> {
	return new Promise((resolve) => {
		platform.confirm(message, resolve);
	});
}

/**
 * Shared hook for delete file operations.
 * Used by both useExplorerKeyboard (DEL key) and useFileContextMenu.
 */
export function useDeleteFiles() {
	const platform = usePlatform();
	const mutation = useLibraryMutation('files.delete');
	const confirmationInFlight = useRef(false);

	const deleteFiles = useCallback(
		async (files: File[], permanent: boolean) => {
			if (files.length === 0) return false;
			if (files.some((f) => !f.sd_path)) return false;
			if (mutation.isPending || confirmationInFlight.current) {
				return false;
			}

			confirmationInFlight.current = true;

			try {
				const label = permanent ? 'Permanently delete' : 'Delete';
				const suffix = permanent ? ' This cannot be undone.' : '';
				const message =
					files.length > 1
						? `${label} ${files.length} items?${suffix}`
						: `${label} "${files[0].name}"?${suffix}`;

				const confirmed = await platformConfirm(platform, message);
				if (!confirmed) return false;

				await mutation.mutateAsync({
					targets: {paths: files.map((f) => f.sd_path)},
					permanent,
					confirm_permanent: permanent && confirmed,
					recursive: true
				});
				return true;
			} catch (err) {
				console.error('Failed to delete:', err);
				alert(`Failed to delete: ${err}`);
				return false;
			} finally {
				confirmationInFlight.current = false;
			}
		},
		[mutation, platform]
	);

	return {deleteFiles, isPending: mutation.isPending};
}
