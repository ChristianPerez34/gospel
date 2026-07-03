import { createCodePlugin } from "@streamdown/code";

/**
 * Shared Streamdown code plugin.
 *
 * Theme pair is tuned for readability on Gospel's tinted-neutral surfaces:
 * - light: `github-light` matches the light surface tokens.
 * - dark: `github-dark-dimmed` softens the harsh default `github-dark` for
 *   extended reading on the near-black dark surface (#090b0d) while keeping
 *   clear token-type discrimination expected in a code-review context.
 *
 * The active theme is selected by Streamdown's `dark:` variant, which is
 * wired to `data-theme` in `src/styles/global.css`.
 */
export const codePlugin = createCodePlugin({
  themes: ["github-light", "github-dark-dimmed"],
});
