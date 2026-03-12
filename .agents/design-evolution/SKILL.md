---
name: design-evolution
description: Use when building complex features, designing architectures, exploring uncertain requirements, or planning systematic refactoring
---

# Design Evolution

## Overview

Structured multi-stage design process that evolves from understanding user intent to detailed implementation planning. Each stage is interactive, with confirmation before proceeding. Less is more - avoid over-engineering.

**Announce at start:** "I'm using the design-evolution skill for structured design thinking."

**Save documents to:** `docs/designs/YYYY-MM-DD-<stage>-<topic>.md`

---

## Stage Detection

Analyze the user's initial request to determine the appropriate entry point:

**Start at Stage 1 (Requirements Analysis) when:**
- User has a rough idea or problem statement
- Requirements are unclear or incomplete
- User wants to explore possibilities
- No existing design documents exist

**Start at Stage 2 (Detailed Design) when:**
- Requirements are already defined and agreed upon
- User has specs or user stories ready
- Previous requirements document exists
- User says "I know what I want, help me design it"

**Start at Stage 3 (Module Breakdown) when:**
- Design/architecture is already defined
- User has high-level design documents
- User says "design is done, help me break it down"
- Moving from design to implementation planning

**Ask for confirmation if uncertain:** "Based on your description, I recommend starting at [Stage X]. Does that sound right?"

---

## Stage 1: Requirements Analysis

### Purpose
Understand user intent, clarify goals, and define what success looks like from the user's perspective. **No code discussion at this stage.**

### Process

**Invoke brainstorming skill:**
```
**REQUIRED SUB-SKILL:** Use superpowers:brainstorming for interactive dialogue
```

Focus on:
- **User goals**: What problem are we solving? For whom?
- **Success criteria**: How will we know this works?
- **Constraints**: Technical, timeline, resource limitations
- **User experience**: What should it feel like to use?
- **Scope boundaries**: What's explicitly OUT of scope

**One question at a time**, multiple choice when possible.

### Output

When requirements are clear, present summary and ask for confirmation:

```markdown
# Requirements Analysis: [Feature Name]

## Problem Statement
[2-3 sentences about what problem we're solving]

## User Goals
- Goal 1
- Goal 2

## Success Criteria
- Criterion 1 (measurable)
- Criterion 2 (measurable)

## Constraints
- Constraint 1
- Constraint 2

## User Experience Vision
[Brief description of ideal user experience]

## Out of Scope
- Thing 1
- Thing 2

---

**Does this capture your requirements correctly?**
```

After confirmation, save to `docs/designs/YYYY-MM-DD-requirements-<topic>.md`

**Transition question:** "Requirements captured. Ready to move to detailed design, or do we need to refine anything?"

---

## Stage 2: Detailed Design

### Purpose
Define high-level architecture, identify domain models, and analyze dependencies. **Focus on structure and relationships, not implementation.**

### Process

**Review requirements document** from Stage 1 (if exists)

**Invoke brainstorming skill** for design decisions:
```
**REQUIRED SUB-SKILL:** Use superpowers:brainstorming for exploring design alternatives
```

**Present approaches with trade-offs:**
- Propose 2-3 architectural approaches
- Explain your recommendation and reasoning
- Discuss domain models and their relationships
- Identify external dependencies

**Dependency analysis with recommendations:**
```
We need [capability]. Options:

1. **Use existing [dependency-name]** (Recommended)
   - Pros: Already in project, proven, consistent patterns
   - Cons: [any limitations]

2. **Add new [alternative-dependency]**
   - Pros: [specific advantages]
   - Cons: New dependency, learning curve

Recommendation: Use existing [dependency-name] because [reasoning].

**Which approach?**
```

**Prioritize reusing existing capabilities** - suggest adding new dependencies only when clearly justified.

### Output

When design is validated, present summary:

```markdown
# Detailed Design: [Feature Name]

## Architecture Overview
[2-3 sentences about the architectural approach]

## Domain Models
- **ModelA**: [purpose and key attributes]
- **ModelB**: [purpose and key attributes]
- Relationships: [how models connect]

## Key Components
- **ComponentA**: [responsibility and interface]
- **ComponentB**: [responsibility and interface]

## Dependencies
- **Existing**: [list of existing dependencies to reuse]
- **New**: [any new dependencies with justification]

## Data Flow
[High-level description of how data moves through the system]

## Error Handling Strategy
[Brief description of error handling approach]

---

**Does this design look right?**
```

After confirmation, save to `docs/designs/YYYY-MM-DD-design-<topic>.md`

**Transition question:** "Design validated. Ready to break down into modules, or any adjustments needed?"

---

## Stage 3: Module Breakdown

### Purpose
Decompose the design into implementation-ready modules with code style alignment and pseudocode flow. **Bridge between design and code.**

### Process

**Review design document** from Stage 2 (if exists)

**Explore project codebase** to understand patterns:
- Search for similar functionality using Task tool with Explore agent
- Identify code conventions (file structure, naming patterns)
- Find reusable abstractions

**Invoke brainstorming skill** for module decisions:
```
**REQUIRED SUB-SKILL:** Use superpowers:brainstorming for interactive module decomposition
```

**Module breakdown strategy:**
- Present 2-3 modularization approaches
- Explain how each aligns with project patterns
- Show pseudocode/comments for clarity
- Focus on single responsibility and loose coupling

**Pseudocode example:**
```python
# module_name.py
"""
Purpose: [what this module does]

Responsibilities:
- [responsibility 1]
- [responsibility 2]
"""

# Step 1: Validate input
# - Check required fields
# - Verify data types

# Step 2: Process core logic
# - [algorithm description]
# - Handle edge cases

# Step 3: Format output
# - Transform to expected structure
# - Add metadata
```

**Code style alignment:**
- Match project's file naming conventions
- Follow existing module structure
- Use similar error handling patterns
- Align with testing approach (if observable)

### Output

When modules are defined, present summary:

```markdown
# Module Breakdown: [Feature Name]

## Modules

### Module 1: [Module Name]
**File**: `path/to/module.py`

**Purpose**: [what this module does]

**Responsibilities**:
- [responsibility 1]
- [responsibility 2]

**Pseudocode Flow**:
```
[pseudocode or comments showing flow]
```

**Dependencies**:
- Internal: [other modules]
- External: [libraries/services]

---

## Module Structure
[Tree structure showing module organization]

## Implementation Notes
- [Code convention notes]
- [Testing considerations]
- [Integration points]

---

**Does this module breakdown work for implementation?**
```

After confirmation, save to `docs/designs/YYYY-MM-DD-modules-<topic>.md`

**Transition question:** "Module breakdown complete. Ready to move to implementation planning with writing-plans skill?"

---

## Overall Flow

```
User Request
    ↓
[Detect Entry Stage]
    ↓
┌─────────────────────────────────────────┐
│  Stage 1: Requirements Analysis         │
│  → Interactive brainstorming            │
│  → Confirm requirements                 │
│  → Save requirements document           │
│  → Ask: Continue to Stage 2?            │
└─────────────────────────────────────────┘
    ↓ [User confirms]
┌─────────────────────────────────────────┐
│  Stage 2: Detailed Design               │
│  → Review requirements                  │
│  → Interactive brainstorming            │
│  → Dependency analysis with recs        │
│  → Confirm design                       │
│  → Save design document                 │
│  → Ask: Continue to Stage 3?            │
└─────────────────────────────────────────┘
    ↓ [User confirms]
┌─────────────────────────────────────────┐
│  Stage 3: Module Breakdown              │
│  → Review design                        │
│  → Explore codebase patterns            │
│  → Interactive brainstorming            │
│  → Confirm modules                      │
│  → Save module document                 │
│  → Ask: Move to implementation?         │
└─────────────────────────────────────────┘
```

**At each stage transition:**
- Present stage output summary
- Ask for confirmation before proceeding
- Allow user to refine or revisit previous stages
- User can stop at any stage

---

## Key Principles

### Less is More
- YAGNI ruthlessly - remove unnecessary features at every stage
- Start simple, add complexity only when justified
- Avoid over-engineering - simplest solution that works

### Interactive Exploration
- One question at a time - don't overwhelm
- Multiple choice preferred - easier to answer
- Present options with your recommendation
- Explain your reasoning clearly

### Progressive Refinement
- Each stage builds on the previous
- Validate before moving forward
- Documents are living artifacts - refine as understanding grows
- Can jump to any stage based on context

### Dependency Pragmatism
- Prefer existing dependencies over new ones
- Reuse project patterns and abstractions
- Justify new dependencies with clear reasons
- Consider long-term maintenance impact

### Code Style Alignment
- Match existing project patterns
- Follow observable conventions
- Learn from similar code in the codebase
- Consistency over cleverness

---

## Common Mistakes

### ❌ Starting at wrong stage
Jumping to implementation when requirements are unclear.
**Fix:** Use stage detection - ask if uncertain.

### ❌ Skipping brainstorming
Presenting completed analysis without interactive dialogue.
**Fix:** Always use brainstorming skill at each stage.

### ❌ Over-documenting
Writing exhaustive documents no one will read.
**Fix:** Keep documents concise - 200-300 words per section.

### ❌ Ignoring existing patterns
Introducing new patterns when project has established ones.
**Fix:** Explore codebase first, align with existing conventions.

### ❌ Adding dependencies unnecessarily
Pulling in new libraries when project has equivalent capability.
**Fix:** Prioritize reuse, justify new dependencies.

### ❌ Forgoing confirmation
Completing all stages without user validation.
**Fix:** Ask for confirmation after each stage before proceeding.

---

## Integration with Other Skills

**After Stage 3, continue with:**
- **superpowers:writing-plans** - Create detailed implementation plan
- **superpowers:using-git-worktrees** - Create isolated workspace

**When requirements are uncertain:**
- **superpowers:brainstorming** - Core dialogue mechanism for all stages

**For implementation:**
- **superpowers:subagent-driven-development** - Execute with reviews
- **superpowers:executing-plans** - Batch execution
