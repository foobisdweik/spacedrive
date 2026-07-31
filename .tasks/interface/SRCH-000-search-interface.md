---
id: SRCH-000
title: Search Interface
status: To Do
assignee: jamiepine
parent: UI-000
priority: Medium
tags: [search, interface]
whitepaper: Section 6.0
---

## Description

Build the search interface with support for full-text search, filters, and semantic search. Displays results in grid/list views with highlighting.

## Features

- Global search bar in top bar
- Real-time search as you type
- Filters (file type, date, size, tags, location)
- Search operators (AND, OR, NOT, quotes)
- Saved searches
- Search history
- Semantic search integration

## Implementation Notes

- Integrates with SEARCH-000 backend
- Uses async search jobs for large queries
- Debounced input for performance
- Results virtualized for large result sets
- Highlight matching terms

## Implemented Foundation

- [x] The Overview search action opens a dedicated `/search` route.
- [x] The search route reuses Explorer results with library-wide scope and an empty-query prompt.
- [x] Search input is focused on route entry and debounced without losing the selected scope.

## Acceptance Criteria

- [ ] Search bar in top bar
- [ ] Real-time results as you type
- [ ] Filter panel with common filters
- [ ] Search operators work correctly
- [ ] Save searches for reuse
- [ ] Search history dropdown
- [ ] Semantic search option (if enabled)
- [ ] Results in grid/list view
- [ ] Click result to open/preview
- [ ] Performance with 100k+ indexed files
