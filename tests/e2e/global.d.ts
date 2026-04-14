// Browser-side globals used inside page.evaluate(() => ...) blocks.
declare const htmx: {
  trigger: (...args: unknown[]) => unknown;
  ajax: (...args: unknown[]) => unknown;
  process: (...args: unknown[]) => unknown;
  [key: string]: unknown;
};
