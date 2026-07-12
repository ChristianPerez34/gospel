Great work on refining the responsive layout! The changes are solid and definitely improve the experience on smaller screens.

Since I am the author of this PR, I am leaving a comment instead of an approval.

### `src/styles/global.css`

I noticed a minor opportunity to DRY up the CSS in the `< 799px` media query. These two rule blocks target the exact same list of classes:

```css
  /* Tighten agent block left margin so content isn't squeezed on small screens */
  .agent-text-block,
  .tool-timeline,
  .agent-error-card,
  .message-actions,
  .running-pill-wrap {
    margin-left: var(--space-3);
  }

  .tool-timeline,
  .agent-text-block,
  .agent-error-card,
  .message-actions,
  .running-pill-wrap {
    width: calc(100% - var(--space-3));
    max-width: 100%;
  }
```

Consider combining them to reduce duplication:

```css
  /* Tighten agent block left margin so content isn't squeezed on small screens */
  .agent-text-block,
  .tool-timeline,
  .agent-error-card,
  .message-actions,
  .running-pill-wrap {
    margin-left: var(--space-3);
    width: calc(100% - var(--space-3));
    max-width: 100%;
  }
```

Everything else looks great! The removal of the `.review-panel` width overrides in favor of `left: 0; right: 0;` in the `< 999px` breakpoint is a nice, clean approach.
