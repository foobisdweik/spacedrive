---
id: LOC-005
title: "Virtual Locations via Pure Hierarchical Model"
status: Done
assignee: jamiepine
parent: LOC-000
priority: High
tags: [core, vdfs, database, refactor]
whitepaper: "Section 4.1.2, 4.3"
---

## Description

A "Location" should not be a rigid, physical path on a disk. It should be a virtual, named pointer to any directory `Entry` in the VDFS.

## Implementation Notes

- [cite_start]The implementation should follow the detailed plan in the **VIRTUAL_LOCATIONS_DESIGN.md** [cite: 5552-5564] document.
- [cite_start]**Drop `relative_path` column** from the `entries` table[cite: 5555].
- [cite_start]**Create `directory_paths` table** to act as a denormalized cache for directory path strings [cite: 5555-5556].
- [cite_start]**Modify `locations` table schema** to store a reference to an `entry_id` instead of a string path[cite: 5559].
- [cite_start]Update indexing and move logic to populate and maintain the `directory_paths` table transactionally [cite: 5560-5561].
- [cite_start]Create a centralized `PathResolver` service to reconstruct full paths on demand[cite: 5563].

## Acceptance Criteria

- [x] A user can create a "Location" that points to any directory `Entry`, regardless of its physical path.
- [x] The `relative_path` column is successfully removed from the database schema.
- [x] Path reconstruction for files is performant, leveraging the `directory_paths` cache.
- [x] [cite_start]Moving a directory correctly updates its path in the cache and triggers a background job to update descendant paths[cite: 5562].

## Implementation

Most of the model landed with the initial schema: `entries` has no `relative_path`, `locations.entry_id` references the root directory entry, `directory_paths` caches directory path strings, and `PathResolver` (`core/src/ops/indexing/path_resolver.rs`) reconstructs and resolves paths. Indexer reconciliation maintains `directory_paths` on moves via `PathResolver::update_descendant_paths`.

Completed in this task:

- `LocationManager::add_location` now reuses an existing directory `Entry` (looked up via `directory_paths`) when the target path is already indexed, instead of inserting a duplicate root entry with `parent_id NULL`. A location is now a true virtual pointer to any directory entry.
- Fixed `PathResolver::update_descendant_paths` to do a prefix-safe rewrite with `substr()` instead of `REPLACE()`/`LIKE`, which corrupted descendant paths containing the old prefix as an interior substring and mismatched on SQL wildcard characters.
- `LocationManager::remove_location` now respects shared entry trees: entries covered by a remaining location are never deleted. Removing a parent location preserves nested locations' subtrees (their roots are detached to standalone roots via `DatabaseStorage::delete_subtree_excluding_in_txn`); removing a nested location deletes only the location row while the parent still covers the entries. Covered by `core/tests/nested_location_test.rs`.
