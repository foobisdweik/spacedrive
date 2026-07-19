/**
 * useNormalizedQuery - Normalized cache with real-time updates
 *
 * A typesafe TanStack Query wrapper providing instant cache updates
 * via filtered WebSocket subscriptions. The counterpart to the Identifiable
 * trait in the Rust core, processing ResourceEvents to update the cache.
 * - Runtime type safety with Valibot
 * - Deep merging with ts-deepmerge
 * - Stable callbacks with React 19 useEvent
 * - Rrror handling with tiny-invariant
 *
 * ## Example
 *
 * ```tsx
 * const { data: files } = useNormalizedQuery({
 *   query: "files.directory_listing",
 *   input: { path: currentPath },
 *   resourceType: 'file',
 *   pathScope: currentPath,
 *   includeDescendants: false, // Exact mode - only direct children
 * });
 * ```
 */

import { useEffect, useMemo, useState, useRef } from "react";
import { useQuery, useQueryClient, QueryClient } from "@tanstack/react-query";
import { useSpacedriveClient } from "./useClient";
import type { Event } from "../generated/types";
import type { SdPath } from "../generated/types";
import { ResourceTypeRegistry } from "../resourceTypeRegistry";
import invariant from "tiny-invariant";
import * as v from "valibot";
import type { Simplify } from "type-fest";

// Types

export type UseNormalizedQueryOptions<I, O = any, TSelected = O> = Simplify<{
	/** Query method to call (e.g., "files.directory_listing") */
	query: string;
	/** Input for the query */
	input: I;
	/** Resource type for event filtering (e.g., "file", "location") */
	resourceType: string;
	/** Whether query is enabled (default: true) */
	enabled?: boolean;
	/** Optional path scope for server-side filtering */
	pathScope?: SdPath;
	/** Whether to include descendants (recursive) or only direct children (exact) */
	includeDescendants?: boolean;
	/** Resource ID for single-resource queries */
	resourceId?: string;
	/** Enable debug logging for this query instance */
	debug?: boolean;
	/** Optional select function to transform query data */
	select?: (data: O) => TSelected;
}>;

// Runtime Validation Schemas (Valibot)

const ResourceChangedSchema = v.object({
	ResourceChanged: v.object({
		resource_type: v.string(),
		resource: v.any(),
		metadata: v.nullish(
			v.object({
				no_merge_fields: v.optional(v.array(v.string())),
				affected_paths: v.optional(v.array(v.any())),
				alternate_ids: v.optional(v.array(v.any())),
			}),
		),
	}),
});

const ResourceChangedBatchSchema = v.object({
	ResourceChangedBatch: v.object({
		resource_type: v.string(),
		resources: v.array(v.any()),
		metadata: v.nullish(
			v.object({
				no_merge_fields: v.optional(v.array(v.string())),
				affected_paths: v.optional(v.array(v.any())),
				alternate_ids: v.optional(v.array(v.any())),
			}),
		),
	}),
});

const ResourceDeletedSchema = v.object({
	ResourceDeleted: v.object({
		resource_type: v.string(),
		resource_id: v.string(),
	}),
});

// Main Hook

/**
 * useNormalizedQuery - Main hook
 */
export function useNormalizedQuery<I, O = any, TSelected = O>(
	options: UseNormalizedQueryOptions<I, O, TSelected>,
) {
	const client = useSpacedriveClient();
	const queryClient = useQueryClient();
	const [libraryId, setLibraryId] = useState<string | null>(
		client.getCurrentLibraryId(),
	);

	// Listen for library changes
	useEffect(() => {
		const handleLibraryChange = (newLibraryId: string) => {
			setLibraryId(newLibraryId);
		};

		client.on("library-changed", handleLibraryChange);
		return () => {
			client.off("library-changed", handleLibraryChange);
		};
	}, [client]);

	const wireMethod = useMemo(
		() => toWireMethod(options.query),
		[options.query],
	);

	// Query key
	const queryKey = useMemo(
		() => [wireMethod, libraryId, options.input],
		[wireMethod, libraryId, JSON.stringify(options.input)],
	);

	// Standard TanStack Query
	const query = useQuery<O, Error, TSelected>({
		queryKey,
		queryFn: async () => {
			invariant(libraryId, "Library ID must be set before querying");
			return await client.execute<I, O>(wireMethod, options.input);
		},
		enabled: (options.enabled ?? true) && !!libraryId,
		select: options.select,
	});

	// Refs for stable access to latest values without triggering re-subscription
	const optionsRef = useRef(options);
	const queryKeyRef = useRef(queryKey);

	// Update refs on every render
	useEffect(() => {
		optionsRef.current = options;
		queryKeyRef.current = queryKey;
	});

	// Serialize pathScope for deep comparison in dependency array
	// This ensures subscription re-runs when path changes, even if object reference stays same
	const pathScopeSerialized = useMemo(
		() => JSON.stringify(options.pathScope),
		[options.pathScope],
	);

	// Event subscription
	// Only re-subscribe when filter criteria change
	// Using refs for event handler to avoid re-subscription on every render
	useEffect(() => {
		if (!libraryId) return;

		// Skip subscription for unscoped file queries (prevent overly broad
		// subscriptions - file resources are too numerous and a global subscription
		// causes massive event spam). The transport's subscribeFiltered only scopes
		// by path_scope, not resourceId, so a resourceId alone can't narrow the
		// subscription server-side - client-side id filtering only reduces cache
		// churn, not incoming event volume. Callers with a single known resource
		// (e.g. FileInspector, QuickPreview) should derive a pathScope once the
		// resource's path is known so they subscribe scoped instead of unscoped.
		if (options.resourceType === "file" && !options.pathScope) {
			return;
		}

		let unsubscribe: (() => void) | undefined;
		let isCancelled = false;

		// Capture current pathScope in closure to prevent stale events from updating wrong query
		const capturedPathScope = options.pathScope;
		const capturedQueryKey = queryKey;

		const handleEvent = (event: Event) => {
			const isDebug = optionsRef.current.debug;
			if (isDebug) {
				console.log(`[useNormalizedQuery] RAW EVENT received:`, typeof event, event);
			}

			// Guard: only process events if pathScope hasn't changed since subscription
			if (
				JSON.stringify(optionsRef.current.pathScope) !==
				JSON.stringify(capturedPathScope)
			) {
				if (isDebug) {
					console.log(`[useNormalizedQuery] STALE pathScope, skipping event`);
				}
				return;
			}

			handleResourceEvent(
				event,
				optionsRef.current,
				capturedQueryKey, // Use captured queryKey, not ref
				queryClient,
				optionsRef.current.debug, // Pass debug flag
			);
		};

		if (options.debug) {
			console.log(`[useNormalizedQuery] SUBSCRIBING: resourceType=${options.resourceType}, pathScope=`, JSON.stringify(options.pathScope), `includeDescendants=${options.includeDescendants ?? false}`);
		}

		client
			.subscribeFiltered(
				{
					resource_type: options.resourceType,
					path_scope: options.pathScope,
					library_id: libraryId,
					include_descendants: options.includeDescendants ?? false,
				},
				handleEvent,
			)
			.then((unsub) => {
				if (isCancelled) {
					if (options.debug) {
						console.log(`[useNormalizedQuery] Subscription created but already cancelled`);
					}
					unsub();
				} else {
					if (options.debug) {
						console.log(`[useNormalizedQuery] Subscription ACTIVE`);
					}
					unsubscribe = unsub;
				}
			})
			.catch((error) => {
				if (!isCancelled && options.debug) {
					console.error("[useNormalizedQuery] Subscription failed", error);
				}
			});

		return () => {
			if (options.debug) {
				console.log(`[useNormalizedQuery] UNSUBSCRIBING`);
			}
			isCancelled = true;
			unsubscribe?.();
		};
	}, [
		client,
		queryClient,
		options.resourceType,
		options.resourceId,
		pathScopeSerialized, // Use serialized version for deep comparison
		options.includeDescendants,
		libraryId,
		// options and queryKey accessed via refs - don't need to be in deps
	]);

	return query;
}

// Event Handling

/**
 * Event handler dispatcher with runtime validation
 *
 * Routes validated events to appropriate update functions.
 * Exported for testing.
 */
export function handleResourceEvent(
	event: Event,
	options: UseNormalizedQueryOptions<any>,
	queryKey: any[],
	queryClient: QueryClient,
	debug?: boolean,
) {
	const wireMethod = toWireMethod(options.query);
	// Skip string events (like "CoreStarted", "CoreShutdown")
	if (typeof event === "string") {
		return;
	}

	// Refresh event - invalidate all queries
	if ("Refresh" in event) {
		if (debug) {
			console.log(
				`[useNormalizedQuery] ${wireMethod} processing Refresh`,
				event,
			);
		}
		queryClient.invalidateQueries();
		return;
	}

	// Single resource changed - validate and process
	if ("ResourceChanged" in event) {
		const result = v.safeParse(ResourceChangedSchema, event);
		if (!result.success) {
			return;
		}

		const { resource_type, resource, metadata } =
			result.output.ResourceChanged;
		if (resource_type === options.resourceType) {
			if (debug) {
				console.log(
					`[useNormalizedQuery] ${wireMethod} processing ResourceChanged`,
					event,
				);
			}
			updateSingleResource(
				// Route through the resource type registry so payloads decode
				// via any registered validator; unknown types pass through.
				ResourceTypeRegistry.decodeOrPassthrough(resource_type, resource),
				metadata,
				queryKey,
				queryClient,
				options,
			);
		}
	}

	// Batch resource changed - validate and process
	else if ("ResourceChangedBatch" in event) {
		const result = v.safeParse(ResourceChangedBatchSchema, event);
		if (!result.success) {
			return;
		}

		const { resource_type, resources, metadata } =
			result.output.ResourceChangedBatch;

		if (
			resource_type === options.resourceType &&
			Array.isArray(resources)
		) {
			if (debug) {
				console.log(
					`[useNormalizedQuery] ${wireMethod} processing ResourceChangedBatch`,
					event,
				);
			}
			updateBatchResources(
				resources.map((resource) =>
					ResourceTypeRegistry.decodeOrPassthrough(resource_type, resource),
				),
				metadata,
				options,
				queryKey,
				queryClient,
			);
		}
	}

	// Resource deleted - validate and process
	else if ("ResourceDeleted" in event) {
		const result = v.safeParse(ResourceDeletedSchema, event);
		if (!result.success) {
			return;
		}

		const { resource_type, resource_id } = result.output.ResourceDeleted;
		if (resource_type === options.resourceType) {
			if (debug) {
				console.log(
					`[useNormalizedQuery] ${wireMethod} processing ResourceDeleted`,
					event,
				);
			}
			deleteResource(resource_id, queryKey, queryClient, resource_type);
		}
	}
}

function toWireMethod(query: string): string {
	invariant(query, "useNormalizedQuery requires a query method");

	if (query.startsWith("query:")) {
		return query;
	}

	invariant(
		!query.startsWith("action:"),
		"useNormalizedQuery only supports queries, remove the action prefix",
	);

	return `query:${query}`;
}

// Batch Filtering

/**
 * Filter batch resources by pathScope for exact mode
 *
 * ## Why This Exists
 *
 * Server-side filtering reduces events by 90%+, but can't split atomic batches.
 * If a batch has 100 files and 1 belongs to our scope, the entire batch is sent.
 * This client-side filter ensures only relevant resources are cached.
 *
 * ## The Critical Bug This Prevents
 *
 * Scenario: Viewing /Desktop, indexing creates batch with:
 * - /Desktop/file1.txt (direct child)
 * - /Desktop/Subfolder/file2.txt (grandchild)
 *
 * Without filtering: Both files appear in /Desktop view
 * With filtering: Only file1.txt appears
 *
 * @param resources - Resources from batch event
 * @param options - Query options
 * @returns Filtered resources for this query scope
 *
 * Exported for testing
 */
export function filterBatchResources(
	resources: any[],
	options: UseNormalizedQueryOptions<any>,
): any[] {
	let filtered = resources;

	// Filter by resourceId (single-resource queries like file inspector)
	if (options.resourceId) {
		filtered = filtered.filter((r: any) => r.id === options.resourceId);
	}

	// Filter by pathScope for file resources in exact mode
	if (
		options.pathScope &&
		options.resourceType === "file" &&
		!options.includeDescendants
	) {
		filtered = filtered.filter((resource: any) => {
			// Get the scope path (must be Physical)
			const scopeStr = (options.pathScope as any).Physical?.path;
			if (!scopeStr) {
				return false; // No Physical scope path
			}

			// Normalize scope: convert Windows backslashes and remove trailing slashes
			const scopeNormalized = String(scopeStr)
				.replace(/\\/g, "/")
				.replace(/\/+$/, "");
			// Only lower-case for Windows paths (case-insensitive FS)
			const isWindowsPath =
				/^[a-zA-Z]:\//.test(scopeNormalized) || scopeNormalized.startsWith("//?/");
			const normalizedScope = isWindowsPath
				? scopeNormalized.toLowerCase()
				: scopeNormalized;

			// Try to find a Physical path - check alternate_paths first, then sd_path
			const alternatePaths = resource.alternate_paths || [];
			const physicalFromAlternate = alternatePaths.find(
				(p: any) => p.Physical,
			);
			const physicalFromSdPath = resource.sd_path?.Physical;

			const physicalPath =
				physicalFromAlternate?.Physical || physicalFromSdPath;

			if (!physicalPath?.path) {
				return false; // No physical path found
			}

			// Normalize Windows backslashes, only lower-case for Windows paths
			const pathNormalized = String(physicalPath.path).replace(/\\/g, "/");
			const pathStr = isWindowsPath ? pathNormalized.toLowerCase() : pathNormalized;

			// Extract parent directory from file path
			const lastSlash = pathStr.lastIndexOf("/");
			if (lastSlash === -1) {
				return false; // File path has no parent directory
			}

			const parentDir = pathStr.substring(0, lastSlash);

			// Only match if parent equals scope (normalized)
			return parentDir === normalizedScope;
		});
	}

	return filtered;
}

// Cache Update Functions

/**
 * Update a single resource using type-safe deep merge
 *
 * Exported for testing
 */
export function updateSingleResource<O>(
	resource: any,
	metadata: any,
	queryKey: any[],
	queryClient: QueryClient,
	options?: UseNormalizedQueryOptions<any>,
) {
	const noMergeFields = metadata?.no_merge_fields || [];

	// Apply client-side filtering if options provided (same as batch)
	let resourcesToUpdate = [resource];
	if (options) {
		resourcesToUpdate = filterBatchResources(resourcesToUpdate, options);
		if (resourcesToUpdate.length === 0) {
			// Resource was filtered out - may have moved out of scope, remove from cache
			if (resource.id) {
				deleteResource(resource.id, queryKey, queryClient, options.resourceType);
			}
			return;
		}
	}

	queryClient.setQueryData<O>(queryKey, (oldData: any) => {
		if (!oldData) {
			// queryFn will deliver correct data. The { files: [...] } fallback assumes
			// list shape, which crashes single-resource queries like libraries.info.
			return undefined;
		}

		// Handle array responses
		if (Array.isArray(oldData)) {
			return updateArrayCache(
				oldData,
				resourcesToUpdate,
				noMergeFields,
			) as O;
		}

		// Handle wrapped responses { files: [...] }
		if (oldData && typeof oldData === "object") {
			return updateWrappedCache(
				oldData,
				resourcesToUpdate,
				noMergeFields,
			) as O;
		}

		return oldData;
	});
}

/**
 * Update batch resources with filtering and deep merge
 *
 * Exported for testing
 */
export function updateBatchResources<O>(
	resources: any[],
	metadata: any,
	options: UseNormalizedQueryOptions<any>,
	queryKey: any[],
	queryClient: QueryClient,
) {
	const noMergeFields = metadata?.no_merge_fields || [];

	// Apply client-side filtering (safety fallback)
	const filteredResources = filterBatchResources(resources, options);

	const wireMethod = toWireMethod(options.query);
	if (options.debug) {
		console.log(`[useNormalizedQuery] ${wireMethod} BATCH: ${resources.length} total, ${filteredResources.length} after filter`);
		if (filteredResources.length === 0 && resources.length > 0) {
			console.log(`[useNormalizedQuery] ${wireMethod} ALL FILTERED OUT! First resource:`, JSON.stringify(resources[0]?.sd_path), `pathScope:`, JSON.stringify(options.pathScope));
		}
	}

	// If all resources were filtered out, they may have moved OUT of scope
	// Remove them from cache if they exist (handles file moves out of current view)
	if (filteredResources.length === 0) {
		for (const resource of resources) {
			if (resource.id) {
				deleteResource(resource.id, queryKey, queryClient, options.resourceType);
			}
		}
		return;
	}

	queryClient.setQueryData<O>(queryKey, (oldData: any) => {
		if (options.debug) {
			console.log(`[useNormalizedQuery] ${wireMethod} setQueryData: oldData has`, Array.isArray(oldData) ? oldData.length : Object.keys(oldData || {}).join(','), `adding ${filteredResources.length} resources`);
		}
		// If the query hasn't returned yet, seed the cache with the event data.
		// This handles the race where the subscription's buffer replay delivers
		// events before the initial query response arrives. Without this, the
		// events would be silently dropped and the UI stays empty.
		if (!oldData) {
			return { files: filteredResources, total_count: filteredResources.length, has_more: false } as O;
		}

		// Handle array responses
		if (Array.isArray(oldData)) {
			return updateArrayCache(
				oldData,
				filteredResources,
				noMergeFields,
			) as O;
		}

		// Handle wrapped responses { files: [...] }
		if (oldData && typeof oldData === "object") {
			return updateWrappedCache(
				oldData,
				filteredResources,
				noMergeFields,
			) as O;
		}

		return oldData;
	});
}

/**
 * Delete a resource from cache
 *
 * Exported for testing
 */
export function deleteResource<O>(
	resourceId: string,
	queryKey: any[],
	queryClient: QueryClient,
	resourceType?: string,
) {
	// Some list queries wrap each row (e.g. tags.search returns
	// { tag, relevance, ... }), so the resource id lives at item[resourceType].id
	// rather than item.id. Match both so deletions actually evict the row.
	const matches = (item: any) =>
		item?.id === resourceId ||
		(resourceType && item?.[resourceType]?.id === resourceId);

	queryClient.setQueryData<O>(queryKey, (oldData: any) => {
		if (!oldData) return oldData;

		if (Array.isArray(oldData)) {
			return oldData.filter((item: any) => !matches(item)) as O;
		}

		if (oldData && typeof oldData === "object") {
			const arrayField = Object.keys(oldData).find((key) =>
				Array.isArray((oldData as any)[key]),
			);

			if (arrayField) {
				return {
					...oldData,
					[arrayField]: (oldData as any)[arrayField].filter(
						(item: any) => !matches(item),
					),
				};
			}
		}

		return oldData;
	});
}

// Cache Update Helpers

/**
 * Extract a normalized physical-path key for a file resource, if it has one.
 *
 * Used to reconcile renames/moves: after a rename the daemon watcher emits a
 * ResourceChanged for the file under a NEW id and NEW path, but the stale entry
 * (old id) is never explicitly deleted. Matching on path lets us collapse the two
 * into one row instead of showing a duplicate. Within a single directory listing a
 * physical path is unique, so path equality reliably means "same file".
 */
function getPhysicalPathKey(resource: any): string | undefined {
	const raw =
		resource?.sd_path?.Physical?.path ??
		resource?.alternate_paths?.find((p: any) => p.Physical)?.Physical?.path;
	if (!raw) return undefined;
	const normalized = String(raw).replace(/\\/g, "/").replace(/\/+$/, "");
	const isWindowsPath =
		/^[a-zA-Z]:\//.test(normalized) || normalized.startsWith("//?/");
	return isWindowsPath ? normalized.toLowerCase() : normalized;
}

/**
 * If an incoming resource shares a physical path with an existing entry that has a
 * DIFFERENT id, replace that entry in place (rename/move reconciliation) and return
 * true. Prevents the stale-old + new duplicate rows reported after rename/move.
 */
function reconcileByPath(
	array: any[],
	resource: any,
	noMergeFields: string[],
): boolean {
	const incomingPath = getPhysicalPathKey(resource);
	if (!incomingPath) return false;

	const staleIndex = array.findIndex(
		(item: any) =>
			item.id !== resource.id && getPhysicalPathKey(item) === incomingPath,
	);
	if (staleIndex === -1) return false;

	// Merge over the stale entry (keeps richer fields like thumbnails if the event
	// is sparse) but adopt the incoming resource's authoritative identity.
	const merged = safeMerge(array[staleIndex], resource, noMergeFields);
	merged.id = resource.id;
	array[staleIndex] = merged;
	return true;
}

/**
 * Update array cache (direct array response)
 */
function updateArrayCache(
	oldData: any[],
	newResources: any[],
	noMergeFields: string[],
): any[] {
	const newData = [...oldData];
	const seenIds = new Set();

	// Update existing items by ID
	for (let i = 0; i < newData.length; i++) {
		const item: any = newData[i];
		const match = newResources.find((r: any) => r.id === item.id);
		if (match) {
			newData[i] = safeMerge(item, match, noMergeFields);
			seenIds.add(match.id);
		}
	}

	// Handle Content entries that represent the same file as an existing Physical entry
	// When content identification happens, a new Content entry is created with a different ID
	// We need to merge it into the existing Physical entry by matching paths
	for (const resource of newResources) {
		if (!seenIds.has(resource.id) && resource.sd_path?.Content) {
			// Try to find existing Physical entry by matching alternate_paths
			const physicalPath = resource.alternate_paths?.find(
				(p: any) => p.Physical,
			)?.Physical?.path;
			if (physicalPath) {
				const existingIndex = newData.findIndex((item: any) => {
					const itemPath =
						item.sd_path?.Physical?.path ||
						item.alternate_paths?.find((p: any) => p.Physical)
							?.Physical?.path;
					return itemPath === physicalPath;
				});

				if (existingIndex !== -1) {
					// Merge Content entry into existing Physical entry
					newData[existingIndex] = safeMerge(
						newData[existingIndex],
						resource,
						noMergeFields,
					);
					seenIds.add(resource.id);
				}
			}
		}
	}

	// Append new items (excluding Content paths that didn't match an existing entry)
	for (const resource of newResources) {
		if (!seenIds.has(resource.id)) {
			// Rename/move reconciliation: if this file already exists under a
			// different id (same physical path), replace it instead of duplicating.
			if (reconcileByPath(newData, resource, noMergeFields)) {
				seenIds.add(resource.id);
				continue;
			}

			// For Content paths: only add if they don't belong to an existing Physical entry
			// Content paths without matching Physical entries are either:
			// 1. Files moved into this directory (have alternate_paths but no match) → ADD
			// 2. Metadata updates for files elsewhere (no relevant alternate_paths) → SKIP
			if (resource.sd_path?.Content) {
				// Skip if no alternate_paths (pure metadata update)
				if (
					!resource.alternate_paths ||
					resource.alternate_paths.length === 0
				) {
					continue;
				}
				// Otherwise, this is a real file that belongs here - add it
			}
			newData.push(resource);
		}
	}

	return newData;
}

/**
 * Update wrapped cache ({ files: [...], locations: [...], etc. })
 */
function updateWrappedCache(
	oldData: any,
	newResources: any[],
	noMergeFields: string[],
): any {
	// First check: if oldData has an id that matches incoming, merge directly
	// This handles single object responses like files.by_id
	const match = newResources.find((r: any) => r.id === oldData.id);
	if (match) {
		return safeMerge(oldData, match, noMergeFields);
	}

	// Second check: wrapped responses like { files: [...] }
	const arrayField = Object.keys(oldData).find((key) =>
		Array.isArray(oldData[key]),
	);

	if (arrayField) {
		const array = [...oldData[arrayField]];
		const seenIds = new Set();

		// Update existing by ID
		for (let i = 0; i < array.length; i++) {
			const item: any = array[i];
			const match = newResources.find((r: any) => r.id === item.id);
			if (match) {
				array[i] = safeMerge(item, match, noMergeFields);
				seenIds.add(match.id);
			}
		}

		// Handle Content entries that represent the same file as an existing Physical entry
		for (const resource of newResources) {
			if (!seenIds.has(resource.id) && resource.sd_path?.Content) {
				// Try to find existing Physical entry by matching alternate_paths
				const physicalPath = resource.alternate_paths?.find(
					(p: any) => p.Physical,
				)?.Physical?.path;
				if (physicalPath) {
					const existingIndex = array.findIndex((item: any) => {
						const itemPath =
							item.sd_path?.Physical?.path ||
							item.alternate_paths?.find((p: any) => p.Physical)
								?.Physical?.path;
						return itemPath === physicalPath;
					});

					if (existingIndex !== -1) {
						// Merge Content entry into existing Physical entry
						array[existingIndex] = safeMerge(
							array[existingIndex],
							resource,
							noMergeFields,
						);
						seenIds.add(resource.id);
					}
				}
			}
		}

		// Append new items (excluding Content paths that didn't match an existing entry)
		for (const resource of newResources) {
			if (!seenIds.has(resource.id)) {
				// Rename/move reconciliation: if this file already exists under a
				// different id (same physical path), replace it instead of duplicating.
				if (reconcileByPath(array, resource, noMergeFields)) {
					seenIds.add(resource.id);
					continue;
				}

				// For Content paths: only add if they don't belong to an existing Physical entry
				// Content paths without matching Physical entries are either:
				// 1. Files moved into this directory (have alternate_paths but no match) → ADD
				// 2. Metadata updates for files elsewhere (no relevant alternate_paths) → SKIP
				if (resource.sd_path?.Content) {
					// Skip if no alternate_paths (pure metadata update)
					if (
						!resource.alternate_paths ||
						resource.alternate_paths.length === 0
					) {
						continue;
					}
					// Otherwise, this is a real file that belongs here - add it
				}

				// Check if resource already exists in the array (by ID)
				const alreadyExists = array.some(
					(item: any) => item.id === resource.id,
				);

				if (alreadyExists) {
					continue;
				}

				// New resource - append it
				array.push(resource);
			}
		}

		return { ...oldData, [arrayField]: array };
	}

	return oldData;
}

/**
 * Safe deep merge for resource updates
 *
 * Arrays are REPLACED (not concatenated) because:
 * - sidecars: Server sends complete list, duplicating would corrupt data
 * - alternate_paths: Same - server is authoritative
 * - tags: Same pattern
 *
 * Only nested objects are deep merged (like content_identity).
 *
 * Exported for testing
 */
export function safeMerge(
	existing: any,
	incoming: any,
	noMergeFields: string[] = [],
): any {
	// Handle null/undefined
	if (incoming === null || incoming === undefined) {
		return existing !== null && existing !== undefined
			? existing
			: incoming;
	}

	// Shallow merge with incoming winning, but deep merge nested objects
	const result: any = { ...existing };

	for (const key of Object.keys(incoming)) {
		const incomingVal = incoming[key];
		const existingVal = existing[key];

		// noMergeFields: incoming always wins
		if (noMergeFields.includes(key)) {
			result[key] = incomingVal;
		}
		// Arrays: replace entirely (don't concatenate)
		else if (Array.isArray(incomingVal)) {
			result[key] = incomingVal;
		}
		// Nested objects: deep merge recursively
		else if (
			incomingVal !== null &&
			typeof incomingVal === "object" &&
			existingVal !== null &&
			typeof existingVal === "object" &&
			!Array.isArray(existingVal)
		) {
			result[key] = safeMerge(existingVal, incomingVal, noMergeFields);
		}
		// Primitives: incoming wins
		else {
			result[key] = incomingVal;
		}
	}

	return result;
}