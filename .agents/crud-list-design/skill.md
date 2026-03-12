---
name: crud-list-design
description: "Use when designing frontend CRUD list pages - standardizes filter configuration, data flow, state management, and form interactions"
---

# CRUD List Design

## Overview

Framework-agnostic design guide for frontend CRUD list pages. Interactively collects project context to generate standardized design documentation covering filter configuration, data interaction patterns, state management, and form data flows.

**Can be used independently** or **called by other design skills** (like `design-evolution`) as a reference standard for frontend list functionality.

**Output:** Structured Markdown design document saved to `docs/designs/crud-lists/YYYY-MM-DD-<resource-name>-list-design.md`

---

## Trigger Conditions

Use this skill when:
- User says "make a list page", "design a CRUD page", "create a table with filters"
- In `design-evolution` Stage 2 when frontend list functionality is involved
- User mentions "filter", "pagination", "search" in the context of list functionality

---

## Execution Process

### Stage 1: Context Collection

Gather project information:
- Framework and component library in use
- Name of the CRUD resource (e.g., "User Management", "Order List")
- Existing list pages to reference for patterns
- API calling patterns (request library, error handling approach)

**Ask questions:**
1. "What framework and UI component library are you using?" (Options: Vue 3 ecosystem, React ecosystem, Other - please specify)
2. "What is the resource name for this list page?" (Open-ended)
3. "Is there an existing list page in the project I can reference for patterns?" (Open-ended file path or description)

### Stage 2: Filter Configuration Collection (Interactive)

For each filter field, ask:

**Basic Type Questions:**
- Field name and label
- Field type: single-select / multi-select / date-range / text-input
- Data source type:
  - **Database distinct**: Backend provides deduplicated options, typically loaded once
  - **Other API**: Separate API endpoint to fetch options, may need lazy loading
  - **Static**: Fixed enum values
- Default value

**Advanced Type Questions (matched to field type):**

**For select fields that might be tree-structured:**
- "Does [field-name] need a tree structure?"
  - If yes, ask:
    - Does data support lazy loading child nodes?
    - Is multi-select supported? If yes, allow selecting leaf nodes only or any node?
    - Default expand level?

**For select fields with many options:**
- "Does [field-name] have a large number of options that need paginated loading?"
  - If yes, ask:
    - Page size
    - Does it support remote search?

**For fields supporting remote search:**
- "Does [field-name] need to support user keyword search?"
  - If yes, ask:
    - Search debounce delay (e.g., 300ms)
    - Minimum search character count (e.g., 2)
    - Should search results be cached?

Generate a structured filter configuration table:

| Field Name | Type | Data Source | API/Field | Advanced Features | Default Value |
|------------|------|-------------|-----------|-------------------|---------------|
| status     | Single-select | Static | - | - | All |
| category   | Multi-select | DB distinct | api/categories/distinct | Tree/Lazy-load/Leaf-only | - |
| userId     | Single-select | Other API | api/users/search | Paginated/Remote search | - |

### Stage 3: List Design

Define table structure and interactions:

**Column Definition:**
- Field name and path
- Data type (text, number, date, badge, etc.)
- Sortable: yes/no
- Formatting requirements (date format, value mapping, etc.)
- Action column configuration (edit, delete, custom actions)

**Pagination Standard:**
- Parameter style: page/pageSize or offset/limit
- Default page size
- Page size options (e.g., [10, 20, 50, 100])

**Query Trigger Timing:**
- Initial load: yes/no
- Filter changes: reset to first page?
- Pagination changes: preserve filters
- Sort changes: reset to first page?

**Permission Control Points:**
- Query permission: where to check
- Action button permissions: edit/delete/export/etc.

**Ask questions:**
1. "What columns should be displayed in the table?" (Interactive list)
2. "Which columns should be sortable?" (Multi-select)
3. "What pagination style does your API use?" (page/pageSize or offset/limit)
4. "Which actions should be available in the operation column?" (Multi-select: Edit, Delete, Export, Custom)
5. "Do any of these actions require permission checks?" (List actions and permissions)

### Stage 4: Form Data Flow Design

Design create/edit form data flows:

**Shared Form Component:**
- Same component used for both create and edit modes
- Mode parameter distinguishes behavior: `mode: 'create' | 'edit'`

**Form Initialization:**
- Create mode: default values (some fields may have API-fetched defaults)
- Edit mode: fetch detail from list item and populate form

**Submission Flow:**
1. Client-side validation
2. Call API endpoint
3. Handle response
   - Success: close modal/redirect, refresh list, show success message
   - Error: display field-level or form-level errors

**Error Handling:**
- Field-level errors: bind to specific form fields
- Form-level errors: display at form top
- Network errors: show generic error message with retry option

**Ask questions:**
1. "Are create and edit forms the same, or do they differ?" (Same form with mode / Different forms)
2. "After successful submission, what should happen?" (Close modal and refresh list / Redirect to list page / Stay on form)
3. "How should validation errors be displayed?" (Field-level / Form-level / Both)

### Stage 5: State Management Design

Define state structure and management approach:

**State Definition:**
```typescript
interface ListState {
  data: Item[]              // List data
  total: number             // Total count for pagination
  filters: FilterValues     // Current filter values
  pagination: {
    page: number
    pageSize: number
  }
  sorting: {
    field: string
    order: 'asc' | 'desc'
  }
  loading: boolean          // Loading state
  error: Error | null       // Error state
}
```

**State Update Patterns:**
- Filter change: update filters, reset page to 1, trigger query
- Pagination change: update pagination, preserve filters, trigger query
- Sort change: update sorting, reset page to 1, trigger query
- Query success: update data, total, clear error, clear loading
- Query error: update error, clear loading, preserve previous data?

**Recommendation based on framework:**
- React: useState + useReducer for complex logic, or Zustand/Jotai for global state
- Vue 3: reactive() + ref(), or Pinia for shared state
- Ask user about their preferred state management approach

**Ask questions:**
1. "What state management approach are you using?" (Options based on framework from Stage 1)
2. "Should query errors preserve the previous successful data, or show empty state?" (Preserve / Empty / Show error only)

---

## Design Document Structure

The generated document includes:

### 1. Overview
- Resource name and description
- Related page routes
- Reference list pages

### 2. Filter Configuration Table

Structured table with all filter fields, types, data sources, and advanced features (tree/pagination/search details).

### 3. Data Interaction Flow

```
┌─────────────────────────────────────────────────────┐
│  Filter Changes                                      │
│  ├─ Basic filters: reset to page 1, trigger query   │
│  └─ Remote search: local update, debounce query     │
│                                                       │
│  Pagination Changes                                  │
│  └─ Page/size change: preserve filters, query       │
│                                                       │
│  Sort Changes                                        │
│  └─ Column sort: reset to page 1, trigger query     │
└─────────────────────────────────────────────────────┘
```

Query interface standard:
- Request: `{ filters, pagination, sorting }`
- Response: `{ data, total, page, pageSize }`

### 4. Table Column Definition

| Column | Field Path | Type | Sortable | Format | Actions |
|--------|------------|------|----------|--------|---------|
| Status | status | badge | Yes | Map display | - |
| Time | createdAt | date | Yes | YYYY-MM-DD | - |
| Actions | - | actions | - | - | Edit/Delete |

### 5. Form Data Flow

Create mode vs Edit mode state transition:
```
Create: { initial defaults } → form validation → submit flow
Edit:   { fetch detail } → form populate → form validation → submit flow
```

Post-submit actions:
- Close modal/redirect to list
- Refresh list data (current page or first page)
- Show success message

### 6. State Management

State structure and update logic (framework-agnostic description)

### 7. Permission Control Points

List all operations requiring permission checks and where to implement them

### 8. Implementation Suggestions (Optional)

Based on user's framework choice, provide relevant code snippet suggestions

---

## Edge Cases Handling

During the design process, also ask about:

**Empty Data:**
- Should empty state be shown? (Yes/No)
- Should it provide a "Create first item" button? (Yes/No)

**Load Failure:**
- Should retry button be shown? (Yes/No)
- Should previous successful data be preserved? (Yes/No)

**Insufficient Permission:**
- Should action buttons be hidden or disabled with tooltip? (Hidden / Disabled + tooltip)

**Batch Operations:**
- Are there batch operations (e.g., batch delete)? (Yes/No)
- If yes, require secondary confirmation? (Yes/No)

---

## Document Generation

**Save location:**
```
docs/designs/crud-lists/YYYY-MM-DD-<resource-name>-list-design.md
```

Examples:
```
docs/designs/crud-lists/2026-01-17-user-management-list-design.md
docs/designs/crud-lists/2026-01-17-order-list-design.md
```

**Versioning:**
If a design for this resource already exists:
- Ask user: "Overwrite existing document or create new version?"
- If new version: append `-v2.md`, `-v3.md`, etc.
- Preserve historical versions for reference

---

## Key Principles

**One Question at a Time:**
- Never overwhelm user with multiple questions
- Prefer multiple choice when possible
- Ask follow-up questions only when relevant (e.g., tree structure only for select fields)

**Framework Agnostic:**
- Use TypeScript interfaces or pseudocode for data structures
- Provide multiple state management options
- Avoid binding to specific APIs (axios vs fetch)

**Recommend Over Prescribe:**
- Suggest using existing project patterns
- Highlight UI structure should follow project conventions
- Focus on standardizing data logic, not UI components

**YAGNI:**
- Don't add features user didn't ask for
- Keep design simple and focused
- Advanced features only when explicitly needed

---

## Integration with Other Skills

**When used standalone:**
- User calls skill directly
- Generate complete design document
- Ask: "Ready to move to implementation?"

**When called by design-evolution:**
- Use in Stage 2 (Detailed Design)
- Output becomes part of overall design document
- Continue to Stage 3 (Module Breakdown)

**After design completion:**
- Ask: "Ready to set up for implementation?"
- Use `superpowers:writing-plans` to create implementation plan
- Use `superpowers:using-git-worktrees` for isolated workspace

---

## Example Interaction Flow

```
User: "I need to design a user management list page"

Assistant: [Invokes crud-list-design skill]

"I'm using the crud-list-design skill for standardized list page design."

[Stage 1: Context Collection]
"What framework and UI library are you using?"
User: "Vue 3 + Element Plus"
"What's the resource name?"
User: "User Management"

[Stage 2: Filter Configuration]
"Let's configure filters. First filter field?"
User: "status"
"Type of status field?"
User: "Single-select"
"Data source?"
User: "Static options: Active, Inactive, Pending"
[Continue for each filter field...]

[Stage 3-5: Complete remaining stages]

[Generate design document]
"Design complete! Saved to docs/designs/crud-lists/2026-01-17-user-management-list-design.md"
"Does this design look correct?"
User: "Yes"

"Ready to move to implementation planning?"
```
