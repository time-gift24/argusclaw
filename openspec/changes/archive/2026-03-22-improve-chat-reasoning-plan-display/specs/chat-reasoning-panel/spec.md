# chat-reasoning-panel

## ADDED Requirements

### Requirement: Reasoning block renders below main text

The system SHALL render the reasoning (思考) content block immediately below the assistant's main text content within an assistant message, replacing the current above-main-text position.

#### Scenario: Reasoning appears below assistant text
- **WHEN** an assistant message contains both text content and reasoning content
- **THEN** the reasoning block is rendered after the main text block in the DOM order

#### Scenario: Reasoning appears below assistant text when only reasoning exists
- **WHEN** an assistant message contains only reasoning content (no text)
- **THEN** the reasoning block is rendered as the sole content below any empty text placeholder

### Requirement: Reasoning block has fixed height with scrolling

The system SHALL display the reasoning block with a fixed maximum height of approximately 150-200px. When content exceeds this height, the block SHALL be scrollable vertically.

#### Scenario: Short reasoning fits without scroll
- **WHEN** the reasoning content height is less than the fixed maximum
- **THEN** the reasoning block expands to fit the content without a visible scrollbar

#### Scenario: Long reasoning overflows with scroll
- **WHEN** the reasoning content height exceeds the fixed maximum height
- **THE** the reasoning block displays a vertical scrollbar, allowing the user to scroll to see all content

### Requirement: Reasoning block auto-scrolls to latest content

The system SHALL automatically scroll the reasoning block to show the most recent content when new reasoning deltas arrive during streaming.

#### Scenario: Auto-scroll during streaming
- **WHEN** new reasoning delta events arrive while streaming
- **THEN** the reasoning block automatically scrolls so the latest content is visible in the viewport

### Requirement: Reasoning block is always expanded

The system SHALL display the reasoning block in an expanded state by default. There SHALL NOT be a collapsible toggle for the reasoning block.

#### Scenario: Reasoning block expanded by default
- **WHEN** an assistant message with reasoning is rendered
- **THEN** the reasoning block is visible and expanded without requiring user interaction
