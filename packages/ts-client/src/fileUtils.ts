import type { ContentKind, File } from "./generated/types";

/**
 * Get the content kind for a file, preferring content_identity.kind if available,
 * falling back to content_kind (identified by extension during ephemeral indexing).
 */
import type { SdPath } from "./generated/types";

/**
 * Derive the parent directory's SdPath from a file's SdPath. Used to scope
 * subscriptions/queries to the containing folder when only a single
 * resource is known up front (e.g. QuickPreview/Inspector opened by id).
 * Returns undefined for root-level paths or path kinds without a parent
 * concept (Content/Sidecar).
 */
export function getParentSdPath(sdPath: SdPath | null | undefined): SdPath | undefined {
	if (!sdPath) return undefined;

	if ("Physical" in sdPath) {
		const fullPath = sdPath.Physical.path;
		const lastSlash = fullPath.lastIndexOf("/");
		if (lastSlash === -1) return undefined;
		return {
			Physical: {
				...sdPath.Physical,
				path: fullPath.substring(0, lastSlash),
			},
		};
	}

	if ("Cloud" in sdPath) {
		const fullPath = sdPath.Cloud.path;
		const lastSlash = fullPath.lastIndexOf("/");
		if (lastSlash === -1) return undefined;
		return {
			Cloud: {
				...sdPath.Cloud,
				path: fullPath.substring(0, lastSlash),
			},
		};
	}

	return undefined;
}

export function getContentKind(file: File | null | undefined): ContentKind {
	return file?.content_identity?.kind ?? file?.content_kind ?? "unknown";
}

/**
 * Get the appropriate kind string for icon resolution.
 * This transforms the content kind into a capitalized string suitable for icon lookup.
 */
export function getFileKindForIcon(file: File | null | undefined): string {
	const contentKind = getContentKind(file);
	const fileKind =
		contentKind && contentKind !== "unknown"
			? contentKind
			: file?.kind === "File"
				? file.extension || "File"
				: file?.kind || "File";
	return fileKind.charAt(0).toUpperCase() + fileKind.slice(1);
}
