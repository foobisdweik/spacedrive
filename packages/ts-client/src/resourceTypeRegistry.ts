/**
 * ResourceTypeRegistry — generic, type-safe deserialization of resource events.
 *
 * The core emits ResourceChanged / ResourceChangedBatch / ResourceDeleted events
 * carrying a `resource_type` string and an untyped JSON payload. This registry
 * maps those identifiers back to the generated client types (see
 * `ResourceTypeMap` in ./generated/types), so cache layers can decode payloads
 * without per-resource switch statements.
 *
 * Every generated resource type is auto-registered with a passthrough decoder
 * (the payload is already the serde serialization of the generated type).
 * Callers may re-register a type with a stricter runtime validator — e.g. a
 * valibot schema — when defense against protocol drift matters.
 */
import {
	RESOURCE_TYPE_NAMES,
	type ResourceType,
	type ResourceTypeMap,
} from "./generated/types";

export type ResourceDecoder<T> = (data: unknown) => T;

export class ResourceTypeRegistry {
	private static decoders = new Map<string, ResourceDecoder<unknown>>();

	/**
	 * Register a decoder for a resource type. Omitting `decoder` installs a
	 * passthrough cast, which is sound for payloads produced by the core's own
	 * serializers.
	 */
	static register<K extends ResourceType>(
		resourceType: K,
		decoder?: ResourceDecoder<ResourceTypeMap[K]>,
	): void {
		this.decoders.set(
			resourceType,
			decoder ?? ((data: unknown) => data as ResourceTypeMap[K]),
		);
	}

	/** Whether a decoder is registered for the given resource type. */
	static isRegistered(resourceType: string): boolean {
		return this.decoders.has(resourceType);
	}

	/** Decode a resource payload. Throws on unknown resource types. */
	static decode<K extends ResourceType>(
		resourceType: K,
		data: unknown,
	): ResourceTypeMap[K];
	static decode(resourceType: string, data: unknown): unknown;
	static decode(resourceType: string, data: unknown): unknown {
		const decoder = this.decoders.get(resourceType);
		if (!decoder) {
			throw new Error(`Unknown resource type: ${resourceType}`);
		}
		return decoder(data);
	}

	/**
	 * Decode a resource payload, returning the raw payload for resource types
	 * that have no registered decoder instead of throwing. Cache updaters use
	 * this so an unknown (newer-core) resource type degrades gracefully.
	 */
	static decodeOrPassthrough(resourceType: string, data: unknown): unknown {
		const decoder = this.decoders.get(resourceType);
		return decoder ? decoder(data) : data;
	}

	/** All resource types with a registered decoder. */
	static registeredTypes(): string[] {
		return [...this.decoders.keys()];
	}
}

// Auto-registration: every resource type in the generated map gets a
// passthrough decoder at module load, so decode() works out of the box.
for (const resourceType of Object.keys(
	RESOURCE_TYPE_NAMES,
) as ResourceType[]) {
	ResourceTypeRegistry.register(resourceType);
}
