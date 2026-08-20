# Contributing

VibeBar contributions should stay reproducible and privacy-preserving. Use sanitized fixtures only, never live account data, and keep credentials and transcript content outside VibeBar.

Before opening a pull request, run:

```sh
npm ci
npm test
```

`npm test` is the complete repository test check and runs both frontend and Rust tests. For release verification, also run `npm run build`, `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`, and `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings`.

Please include tests for behavior changes and keep new collectors bounded, read-only, and free of private user data. Do not add browser cookies, access tokens, prompts, responses, local database copies, or generated bundles to Git.
