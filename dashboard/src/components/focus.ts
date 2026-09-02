/** Keyboard focus ring for interactive controls: the same shape as the
 * global `:focus-visible` rule in app.css, applied per element so a control
 * that overrides `outline` (or is rendered outside the stylesheet, as in
 * tests) still shows one. */
export const FOCUS_RING = "focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-prefill";
