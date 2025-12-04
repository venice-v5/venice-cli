## Background

This is the repository for the Venice CLI. Venice is an independent port of Micropython to the VEX V5. You may know of a first-party Micropython runtime through VEXCode, which is maintained by VEX; **Venice is separate**.

For reference, a Venice program is uploaded to the VEX V5 Brain as two parts: 1) the Venice runtime, which is compiled once for every version of Venice and is a binary which loads the Python bytecode of the program from a particular memory address and runs it, and 2) the actual Python bytecode. The bytecode is currently stored using a custom VPT (Venice Program Table) format to map module identifiers to file bytecode.

## Task

Venice is currently in the process of a major refactor. Previously, the plan was to:
* have a `venice` Python package which just hosted stubs for the Devices API. Users could add the `venice` package to their Venice projects for improved typesafety.
* a separate `venice-cli` Python package contained the Venice CLI, which, when building, would download the runtime for Venice from GitHub Releases to upload to the brain. In face, the CLI would download the runtime *every time* to cross-check the checksums and ensure the runtime on the brain had not been replaced.

A newer plan is designed to better logically couple the packages, like so:
* the `venice` Python package contains the stubs as usual, for typesafety, but importantly, also packages the runtime binary as an asset. Along with the stubs, it exposes a method like `_dangerous_get_binary_path()` which is not for use by users.
* the `venice-cli` Python package depends on `venice`, and imports it to get the runtime binary. It can then use this directly for uploading, checksums, etc.
* Users will run `uv add venice` to add Venice, then run `uv add --dev venice-cli` to add the CLI, which resolves the version of the Venice package previously added. Then, users can simply run `uv venice-cli upload` and similar.

Here's the progress so far on the plan:
- [X] Set up CI for building a `venice` wheel with stubs, runtime binary, and "hidden" methods to access the binary.
- [ ] Update the CLI to remove any URL fetches and instead rely on the `venice` package to provide the binary path.

Your goal is to finish the second step of this plan. Note that the CLI is written in Rust, with a Python package provided through Maturin; you need to update the `call` function that Rust exposes to also take in an optional binary path and version. The Python side will need to have the `venice` dependency added, and try to import `venice` and call `_dangerous_DO_NOT_TOUCH_YOU_WILL_GET_ELECTROCUTED_get_binary_path` and `_dangerous_DO_NOT_TOUCH_YOU_WILL_GET_ELECTROCUTED_get_version` (both of which take no args and return a string) to pass into `call`. If the venice import fails, then pass in None.

The CLI should also support `--raw-binary` mode, but ONLY in development builds. The raw mode enables testing runtime binaries without making a new wheel. When `--raw-binary=/path/to/binary` is enabled, any binary path from the venice package should be ignored, and instead, the binary path provided by the user should be used; the version should be assumed to be 0.1.0. This functionality should be implemented on the Rust side.
