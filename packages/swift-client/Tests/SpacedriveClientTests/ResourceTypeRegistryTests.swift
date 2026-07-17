import XCTest

@testable import SpacedriveClient

final class ResourceTypeRegistryTests: XCTestCase {
    private struct TestResource: CacheableResource, Equatable {
        static let resourceType = "test_resource"

        let id: String
        let name: String
    }

    func testGeneratedResourcesRegisterAutomatically() {
        XCTAssertEqual(
            ResourceTypeRegistry.shared.registeredTypes,
            GeneratedResources.allResourceTypes.sorted()
        )
        XCTAssertTrue(ResourceTypeRegistry.shared.isRegistered("file"))
    }

    func testRegistrationAndDataDecoding() throws {
        let registry = ResourceTypeRegistry()
        registry.register(TestResource.self)

        let data = Data(#"{"id":"resource-1","name":"Document"}"#.utf8)
        let decoded = try registry.decode(resourceType: TestResource.resourceType, from: data)

        XCTAssertEqual(decoded as? TestResource, TestResource(id: "resource-1", name: "Document"))
    }

    func testResourceEventJsonValueDecoding() throws {
        let registry = ResourceTypeRegistry()
        registry.register(TestResource.self)

        let payload = JsonValue.object([
            "id": .string("resource-2"),
            "name": .string("Photo"),
        ])
        let decoded = try registry.decode(resourceType: TestResource.resourceType, from: payload)

        XCTAssertEqual(decoded as? TestResource, TestResource(id: "resource-2", name: "Photo"))
    }

    func testUnknownResourceTypeThrows() {
        let registry = ResourceTypeRegistry()

        XCTAssertThrowsError(
            try registry.decode(resourceType: "missing", from: Data("{}".utf8))
        ) { error in
            XCTAssertEqual(
                error as? ResourceTypeRegistryError,
                .unknownResourceType("missing")
            )
        }
    }
}
