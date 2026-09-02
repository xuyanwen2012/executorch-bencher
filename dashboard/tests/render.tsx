// Minimal React test harness over the preloaded happy-dom window: mounts an
// element with `act`, offers a polling `waitFor`, and fires the native
// events React listens for. No testing-library is installed; this is the
// small subset the component tests need.
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";

export { act };

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

export interface Mounted {
  container: HTMLElement;
  rerender(element: ReactElement): Promise<void>;
  unmount(): Promise<void>;
}

export async function mount(element: ReactElement): Promise<Mounted> {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  await act(async () => {
    root.render(element);
  });
  return {
    container,
    async rerender(next) {
      await act(async () => {
        root.render(next);
      });
    },
    async unmount() {
      await act(async () => {
        root.unmount();
      });
      container.remove();
    },
  };
}

/** Lets timers and promises run for `ms`, inside `act` so React flushes. */
export async function tick(ms = 0): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

/** Polls `predicate` until it holds or `timeoutMs` passes. */
export async function waitFor(predicate: () => boolean, timeoutMs = 2000): Promise<void> {
  const start = Date.now();
  while (!predicate()) {
    if (Date.now() - start > timeoutMs) throw new Error("waitFor: condition not met in time");
    await tick(10);
  }
}

/** Types `value` into a controlled input the way a user would: through the
 * native value setter (so React's value tracker notices) and an `input`
 * event. */
export async function type(input: HTMLInputElement, value: string): Promise<void> {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  await act(async () => {
    if (setter) setter.call(input, value);
    else input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

export async function select(control: HTMLSelectElement, value: string): Promise<void> {
  const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")?.set;
  await act(async () => {
    if (setter) setter.call(control, value);
    else control.value = value;
    control.dispatchEvent(new Event("change", { bubbles: true }));
  });
}

export async function click(element: Element): Promise<void> {
  await act(async () => {
    element.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
  });
}

export async function keydown(element: Element, key: string): Promise<void> {
  await act(async () => {
    element.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  });
}
