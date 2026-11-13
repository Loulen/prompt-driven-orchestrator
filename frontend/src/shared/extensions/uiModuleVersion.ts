/**
 * The version of the shared module `@pdo/extension-ui` (ADR-0081, amendment
 * #32): the version of `pdo-extension-api`, whose major it is. A bundle built
 * against another major is not loaded (`store.ts`), as for React. A structural
 * test (`crates/tidy/tests/shared_module.rs`, `make fast`) fails when these
 * lines drift from the version of `crates/pdo-extension-api/Cargo.toml`.
 */
export const UI_MODULE_VERSION = "0.27.0";
export const UI_MODULE_MAJOR = 0;
