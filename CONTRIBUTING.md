# Contributing

Issues and pull requests are welcome.

## Development setup

Install Node.js 18 or later, stable Rust, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```bash
npm ci
npm test
npm run build
cargo test --manifest-path src-tauri/crates/events-server/Cargo.toml
```

Use `npm run tauri dev` for an interactive desktop build. The bundled `xangi` pet is used automatically.

## Pull requests

- Keep each pull request focused on one behavior or closely related set of changes.
- Add a regression test for bug fixes.
- Update `README.md`, `README.en.md`, or `docs/` when user-visible behavior changes.
- Do not commit tokens, local xangi URLs, generated runtime state, or personal pet assets without permission.
- Run the targeted tests and build commands relevant to the change.

GitHub Actions builds installable bundles for macOS, Windows, and Linux on non-documentation pull requests.

## Third-party dependencies

This project is Apache-2.0 licensed. Check the license of any new dependency before adding it. When Rust dependencies change, regenerate the bundled notices:

```bash
cargo install cargo-about --locked --features cli
./scripts/gen-licenses.sh
```

Commit the updated `src-tauri/THIRD_PARTY_LICENSES.html` with the dependency change.
