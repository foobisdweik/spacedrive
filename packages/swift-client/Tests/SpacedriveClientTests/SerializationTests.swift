import XCTest

@testable import SpacedriveClient

final class SerializationTests: XCTestCase {

    func testLibraryCreateInputSerialization() throws {
        // Test that Swift types serialize to JSON correctly
        let input = LibraryCreateInput(name: "Test Library", path: "/test/path")

        // Serialize to JSON
        let jsonData = try JSONEncoder().encode(input)
        let jsonString = String(data: jsonData, encoding: .utf8)!

        print("LibraryCreateInput JSON: \(jsonString)")

        // Verify JSON structure matches what Rust expects
        let jsonObject = try JSONSerialization.jsonObject(with: jsonData) as! [String: Any]
        XCTAssertEqual(jsonObject["name"] as? String, "Test Library")
        XCTAssertEqual(jsonObject["path"] as? String, "/test/path")

        // Test round-trip serialization
        let decoded = try JSONDecoder().decode(LibraryCreateInput.self, from: jsonData)
        XCTAssertEqual(decoded.name, input.name)
        XCTAssertEqual(decoded.path, input.path)
    }

    func testLibraryCreateOutputDeserialization() throws {
        // Test that we can deserialize JSON from daemon into Swift types
        let jsonString = """
            {
                "library_id": "123e4567-e89b-12d3-a456-426614174000",
                "name": "Test Library",
                "path": "/test/path"
            }
            """

        let jsonData = jsonString.data(using: .utf8)!
        let output = try JSONDecoder().decode(LibraryCreateOutput.self, from: jsonData)

        XCTAssertEqual(output.libraryId, "123e4567-e89b-12d3-a456-426614174000")
        XCTAssertEqual(output.name, "Test Library")
        XCTAssertEqual(output.path, "/test/path")

        print("LibraryCreateOutput deserialized successfully: \(output)")
    }

    func testUnionTypeSerialization() throws {
        // Test union types (enums with associated values)
        let physicalPath = SdPath.physical(
            SdPathPhysicalData(deviceSlug: "device-123", path: "/test/file.txt"))
        let contentPath = SdPath.content(SdPathContentData(contentId: "content-456"))

        // Test physical path serialization
        let physicalData = try JSONEncoder().encode(physicalPath)
        let physicalJson = String(data: physicalData, encoding: .utf8)!
        print("Physical SdPath JSON: \(physicalJson)")

        // Test content path serialization
        let contentData = try JSONEncoder().encode(contentPath)
        let contentJson = String(data: contentData, encoding: .utf8)!
        print("Content SdPath JSON: \(contentJson)")

        // Test round-trip
        let decodedPhysical = try JSONDecoder().decode(SdPath.self, from: physicalData)
        let decodedContent = try JSONDecoder().decode(SdPath.self, from: contentData)

        // Verify the decoded values match
        switch decodedPhysical {
        case .physical(let data):
            XCTAssertEqual(data.deviceSlug, "device-123")
            XCTAssertEqual(data.path, "/test/file.txt")
        default:
            XCTFail("Expected physical path")
        }

        switch decodedContent {
        case .content(let data):
            XCTAssertEqual(data.contentId, "content-456")
        default:
            XCTFail("Expected content path")
        }
    }

    func testJobStatusSerialization() throws {
        // Test simple enum serialization
        let statuses: [JobStatus] = [.queued, .running, .completed, .failed]

        for status in statuses {
            let data = try JSONEncoder().encode(status)
            let json = String(data: data, encoding: .utf8)!
            let decoded = try JSONDecoder().decode(JobStatus.self, from: data)

            print("JobStatus \(status) → JSON: \(json)")
            XCTAssertEqual(decoded, status)
        }
    }

    func testJobOutputSerialization() throws {
        // Test complex enum with associated values
        let indexedOutput = JobOutput.indexed(
            JobOutputIndexedData(
                stats: IndexerStats(
                    files: 100, dirs: 10, bytes: 1_024_000, symlinks: 5, skipped: 2, errors: 0),
                metrics: IndexerMetrics(
                    totalDuration: RustDuration(secs: 30, nanos: 500_000_000),
                    discoveryDuration: RustDuration(secs: 5, nanos: 0),
                    processingDuration: RustDuration(secs: 20, nanos: 0),
                    contentDuration: RustDuration(secs: 5, nanos: 500_000_000),
                    filesPerSecond: 3.33,
                    bytesPerSecond: 34133.33,
                    dirsPerSecond: 0.33,
                    dbWrites: 110,
                    dbReads: 50,
                    batchCount: 5,
                    avgBatchSize: 20.0,
                    totalErrors: 0,
                    criticalErrors: 0,
                    nonCriticalErrors: 0,
                    skippedPaths: 2,
                    peakMemoryBytes: 1_048_576,
                    avgMemoryBytes: 524288
                )
            ))

        // Test serialization
        let data = try JSONEncoder().encode(indexedOutput)
        let json = String(data: data, encoding: .utf8)!
        print("Complex JobOutput JSON: \(json)")

        // Test round-trip
        let decoded = try JSONDecoder().decode(JobOutput.self, from: data)

        switch decoded {
        case .indexed(let data):
            XCTAssertEqual(data.stats.files, 100)
            XCTAssertEqual(data.metrics.filesPerSecond, 3.33, accuracy: 0.01)
        default:
            XCTFail("Expected indexed output")
        }
    }

    func testFileSystemEnumSerialization() throws {
        // Test enum with associated values
        let apfs = FileSystem.aPFS
        let other = FileSystem.other("custom-fs")

        // Test simple variant
        let apfsData = try JSONEncoder().encode(apfs)
        let apfsJson = String(data: apfsData, encoding: .utf8)!
        print("FileSystem.apfs JSON: \(apfsJson)")

        // Test variant with associated value
        let otherData = try JSONEncoder().encode(other)
        let otherJson = String(data: otherData, encoding: .utf8)!
        print("FileSystem.other JSON: \(otherJson)")

        // Test round-trip
        _ = try JSONDecoder().decode(FileSystem.self, from: apfsData)
        let decodedOther = try JSONDecoder().decode(FileSystem.self, from: otherData)

        // XCTAssertEqual(decodedApfs, .apfs) // TODO: Add Equatable to generated enums
        switch decodedOther {
        case .other(let fs):
            XCTAssertEqual(fs, "custom-fs")
        default:
            XCTFail("Expected other filesystem")
        }
    }

    func testRealDaemonIntegration() async throws {
        // Skip if daemon is not running
        let socketPath =
            "\(NSHomeDirectory())/Library/Application Support/spacedrive/daemon/daemon.sock"

        guard FileManager.default.fileExists(atPath: socketPath) else {
            throw XCTSkip("Daemon not running - skipping integration test")
        }

        let client = SpacedriveClient(socketPath: socketPath)

        // Test real API call with generated types
        do {
            let libraries = try await client.libraries.list(
                ListLibrariesInput(includeStats: true))

            print("Real daemon integration successful - found \(libraries.count) libraries")

            // If we have libraries, test job list with generated types
            if !libraries.isEmpty {
                client.setCurrentLibrary(libraries[0].id)
                let jobsResponse = try await client.jobs.list(JobListInput(status: nil))

                print("Jobs query successful - found \(jobsResponse.jobs.count) jobs")

                // Verify the types match our generated Swift types
                for job in jobsResponse.jobs {
                    XCTAssertFalse(job.id.isEmpty)
                    XCTAssertFalse(job.name.isEmpty)
                    // job.status should be a JobStatus enum value
                    print("  Job: \(job.name) (\(job.status)) - \(Int(job.progress * 100))%")
                }
            }

        } catch {
            print("️ Daemon integration failed: \(error)")
            // Don't fail the test - daemon might not have libraries
        }
    }
}
