# ADR 0006: Use Fluent-Inspired Nota Design Tokens

- Status: Accepted
- Date: 2026-08-11
- Last updated: 2026-08-11
- Decision owners: Nota maintainers

## Context

Nota's interface accumulated page-local font sizes, foreground colors,
surfaces, borders, radii, and shadows. Equivalent controls therefore developed
different visual hierarchy and interaction affordances. Nota is a Windows 11
desktop product, already uses React and CSS, and has begun using accessible
Radix primitives.

## Decision

Nota will own a semantic design-token layer in `src/design-tokens.css` and a
durable usage specification in `docs/design-system.md`.

The initial type ramp and spacing rhythm follow Fluent 2's Windows guidance.
Nota retains its own warm-neutral and jade visual identity. Components consume
semantic Nota tokens; they do not consume hard-coded colors or establish
page-local type ramps. Radix primitives may supply behavior and accessibility,
but Nota tokens supply their visual treatment.

Automated checks reject literal colors and literal font sizes outside the
token source of truth.

## Alternatives Considered

- Adopt Fluent UI React wholesale. Rejected because it would require a broad
  component rewrite and would couple Nota's product identity to Microsoft's
  component styling rather than using Fluent as a Windows-oriented baseline.
- Adopt Radix Themes wholesale. Rejected because Nota already has a mature
  application layout and needs an owned brand language rather than another
  product's complete visual defaults.
- Keep page-local CSS and document preferred values only. Rejected because a
  non-executable convention would not prevent the inconsistency that prompted
  this decision.
- Replace CSS with a CSS-in-JS or utility framework. Rejected because tokens
  and shared components solve the consistency problem without a styling-stack
  migration.

## Consequences

- Visual changes require a semantic role and an existing or new token.
- Token additions and meaning changes require documentation review.
- Existing CSS must migrate to the shared type and color roles.
- Nota can add dark or high-contrast themes later by remapping semantic tokens
  without rewriting page styles.
- Some layout dimensions remain component-owned where they express geometry
  rather than reusable spacing.

## Compatibility and Evolution

Token names are internal source compatibility contracts. Values may be tuned
without renaming consumers. Renaming or removing a token requires migrating
all consumers in the same change. A future theme or component library may
implement the same semantic contract; replacing that contract requires a new
ADR.
