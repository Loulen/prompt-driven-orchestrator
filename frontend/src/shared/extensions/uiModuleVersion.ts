/**
 * The version of the shared module `@pdo/extension-ui` (ADR-0081, amendment
 * #32): the version of `pdo-extension-api`, whose major it is. A bundle built
 * against another major is not loaded (`store.ts`), as for React. A daemon test
 * (`crates/pdo/src/extensions.rs`) fails when these lines drift from the API.
 */
export const UI_MODULE_VERSION = "0.9.0";
export const UI_MODULE_MAJOR = 0;
