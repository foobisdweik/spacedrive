import Foundation

/// A resource that can be decoded from resource events and cached by identity.
///
/// Conforming types are the generated Spacedrive domain types (File, Location,
/// Tag, ...). Conformances are emitted by codegen into
/// `ResourceTypeRegistry+Generated.swift` — do not add them by hand.
public protocol CacheableResource: Codable, Identifiable {
    /// The wire identifier carried in resource events (e.g. "file").
    static var resourceType: String { get }
}

public enum ResourceTypeRegistryError: Error, Equatable {
    case unknownResourceType(String)
}

/// Maps `resource_type` identifiers from resource events
/// (ResourceChanged / ResourceChangedBatch / ResourceDeleted) to decoders for
/// their generated Swift types, enabling generic deserialization without
/// per-resource switch statements.
///
/// The shared registry is populated from generated metadata on first access.
/// Standalone registry instances can opt into the same set with
/// `GeneratedResources.registerAll(into:)`.
public final class ResourceTypeRegistry: @unchecked Sendable {
    /// Process-wide registry, populated from generated resource metadata on
    /// first access so event consumers do not need a separate startup step.
    public static let shared: ResourceTypeRegistry = {
        let registry = ResourceTypeRegistry()
        GeneratedResources.registerAll(into: registry)
        return registry
    }()

    private var decoders: [String: (Data) throws -> any CacheableResource] = [:]
    private let lock = NSLock()

    public init() {}

    /// Register a resource type. Its `resourceType` identifier becomes
    /// decodable via `decode(resourceType:from:)`.
    public func register<T: CacheableResource>(_ type: T.Type) {
        lock.lock()
        defer { lock.unlock() }
        decoders[T.resourceType] = { data in
            try JSONDecoder().decode(T.self, from: data)
        }
    }

    /// Whether a decoder is registered for the given resource type.
    public func isRegistered(_ resourceType: String) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return decoders[resourceType] != nil
    }

    /// All resource types with a registered decoder.
    public var registeredTypes: [String] {
        lock.lock()
        defer { lock.unlock() }
        return Array(decoders.keys).sorted()
    }

    /// Decode a resource payload by its wire identifier.
    /// - Throws: `ResourceTypeRegistryError.unknownResourceType` when no
    ///   decoder is registered, or any `DecodingError` from the payload.
    public func decode(resourceType: String, from data: Data) throws -> any CacheableResource {
        lock.lock()
        let decoder = decoders[resourceType]
        lock.unlock()

        guard let decoder else {
            throw ResourceTypeRegistryError.unknownResourceType(resourceType)
        }
        return try decoder(data)
    }

    /// Decode a resource payload from an already-parsed JSON object, as
    /// carried inside resource events.
    public func decode(resourceType: String, fromJSONObject object: Any) throws -> any CacheableResource {
        let data = try JSONSerialization.data(withJSONObject: object)
        return try decode(resourceType: resourceType, from: data)
    }

    /// Decode the generated `JsonValue` payload carried by resource events.
    public func decode(resourceType: String, from value: JsonValue) throws -> any CacheableResource {
        let data = try JSONEncoder().encode(value)
        return try decode(resourceType: resourceType, from: data)
    }
}
