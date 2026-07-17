import {
	RESOURCE_TYPE_NAMES,
	type File,
} from "../generated/types";
import { ResourceTypeRegistry } from "../resourceTypeRegistry";

describe("ResourceTypeRegistry", () => {
	it("auto-registers every generated resource type", () => {
		expect(ResourceTypeRegistry.registeredTypes().sort()).toEqual(
			Object.keys(RESOURCE_TYPE_NAMES).sort(),
		);
		expect(ResourceTypeRegistry.isRegistered("file")).toBe(true);
	});

	it("uses a registered decoder", () => {
		const payload = { id: "file-1", name: "Photo.jpg" };
		ResourceTypeRegistry.register("file", (data) => ({
			...(data as File),
			name: "decoded",
		}));

		expect(ResourceTypeRegistry.decode("file", payload)).toMatchObject({
			id: "file-1",
			name: "decoded",
		});
	});

	it("throws for an unknown resource type", () => {
		expect(() => ResourceTypeRegistry.decode("missing", {})).toThrow(
			"Unknown resource type: missing",
		);
	});

	it("passes unknown future resource types through for cache updates", () => {
		const payload = { id: "future-1" };
		expect(
			ResourceTypeRegistry.decodeOrPassthrough("future_resource", payload),
		).toBe(payload);
	});
});
