import { useNormalizedQuery } from "@sd/ts-client";
import { SpaceItem } from "./SpaceItem";
import { GroupHeader } from "./GroupHeader";

interface LocationsGroupProps {
  isCollapsed: boolean;
  onToggle: () => void;
  sortableAttributes?: any;
  sortableListeners?: any;
}

export function LocationsGroup({
  isCollapsed,
  onToggle,
  sortableAttributes,
  sortableListeners,
}: LocationsGroupProps) {
  const { data: locationsData } = useNormalizedQuery({
    query: "locations.list",
    input: null, // Unit struct serializes as null, not {}
    resourceType: "location",
  });

  const locations = locationsData?.locations ?? [];
  const activeLocations = locations.filter((loc: any) => loc.is_available);

  return (
    <div>
      <GroupHeader
        label="Locations"
        isCollapsed={isCollapsed}
        onToggle={onToggle}
        sortableAttributes={sortableAttributes}
        sortableListeners={sortableListeners}
      />

      {/* Items */}
      {!isCollapsed && (
        <div className="space-y-0.5">
          {activeLocations.map((location: any, index: number) => (
            <SpaceItem
              key={location.id}
              item={location}
              allowInsertion={false}
              isLastItem={index === activeLocations.length - 1}
            />
          ))}
        </div>
      )}
    </div>
  );
}
