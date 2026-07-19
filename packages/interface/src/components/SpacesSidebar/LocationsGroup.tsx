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
          {/*
            Render unavailable locations too (dimmed), rather than filtering them
            out. Hiding them made a location whose folder went missing silently
            disappear; keeping it visible and navigable lets the user open it and
            reach the missing-path recovery view.
          */}
          {locations.map((location: any, index: number) => (
            <div
              key={location.id}
              className={location.is_available ? undefined : "opacity-50"}
              title={
                location.is_available
                  ? undefined
                  : "This location's folder is missing. Open it to relink or remove."
              }
            >
              <SpaceItem
                item={location}
                allowInsertion={false}
                isLastItem={index === locations.length - 1}
              />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
