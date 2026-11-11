# Contributing to OpenIDEA-CfxLUA

Thanks for helping make FiveM and RedM development better in OpenIDEA. Bug reports, feature ideas, documentation
fixes and pull requests are all welcome.

## Reporting a bug

Open an [issue](https://github.com/SensityTech/OpenIDEA-CfxLUA/issues/new/choose) with:

- the OpenIDEA and plugin versions (**Settings › Plugins**);
- a minimal resource that shows the problem (an `fxmanifest.lua` and the script), what you expected and what happened;
- for slowness or a crash, the plugin's line in **Settings › Plugins** (activations, p95 latency, crashes) and the
  lines of the IDE log mentioning `sensity.cfxlua`.

Security issues: see [SECURITY.md](SECURITY.md), not a public issue.

## Development setup

- Rust stable (edition 2024) with the WebAssembly target: `rustup target add wasm32-wasip2`
  (Windows without Visual Studio Build Tools: the `stable-x86_64-pc-windows-gnu` toolchain works).
- `clippy` and `rustfmt`: `rustup component add clippy rustfmt`.
- [OpenIDEA](https://github.com/l0ng-ai/openidea) 0.1.40 or newer to try the plugin.

| Command | |
|---|---|
| `cargo test --lib` | Unit tests (native, against the SDK's mock host). |
| `cargo clippy --all-targets -- -D warnings` | Lints; CI fails on any warning. |
| `cargo fmt --all` | Formatting (`rustfmt.toml`, 120 columns). |
| `scripts/build.sh --pack` (`scripts\build.ps1 -Pack`) | Builds `dist/plugin.wasm` and the `.oiplugin` package. |
| `scripts\build.ps1 -Link` | Links the repository folder into OpenIDEA; **Settings › Plugins › Reload** after each build. |

## How the code is organised

| Path | |
|---|---|
| `src/lib.rs` | Entry point: providers (completion, definition, gutter, quick fixes), events, commands, the tool window, the status bar. |
| `src/feature.rs` | The `Feature` trait every area implements, `Doc`, `Ctx`, `Problem`, `Fix`. |
| `src/lua/` | CfxLua lexer, parser and syntax tree, scopes, documentation comments (`doc/`), and a light structural view (`Source`, chains, calls). |
| `src/ty/` | The types of Lua values: the model, how a type is shown, the types written in documentation comments. |
| `src/editor/` | What the editor shows of a Lua file: semantic colours, folding, structure, occurrences. |
| `src/project/` | Resources, manifest entries, FXServer globs, client/server/shared side, game. |
| `src/workspace.rs` | The project files the plugin has read; areas only read the project through it. |
| `src/<area>/` | `natives`, `runtime`, `manifest`, `escrow`, `statebags`, `events`, `nui`, `values`, `coords`, `perf`, `syntax`, `docs`. |
| `web/` | The tool window page (rendering only; searching and HTML are built in Rust). |
| `data/` | Bundled data: natives snapshots, guides, catalogs, value tables, runtime stubs. |
| `plugin.toml`, `settings.schema.json` | Plugin manifest and settings page. |

Two rules matter more than any other:

1. **Providers run on the editing path.** OpenIDEA drops a completion, definition or gutter result that takes more
   than 50 ms, shared by every plugin. Project-wide tables are built in `Feature::index_file` / `Feature::indexed`
   (background work), never in a provider. A completion is asked at every keystroke: it looks at the tokens around
   the caret before it parses anything, and the edited document only reaches `index_file` when a request reads
   those tables (`Feature::completes_from_index` says whether a completion does).
2. **Areas never read the project from the host.** They get a `Workspace`, so every feature is unit-tested in memory
   with `testing::Fixture`:

```rust
let fx = Fixture::new(&[
    ("shop/fxmanifest.lua", "fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client/*.lua'"),
    ("shop/client/main.lua", "AddEventHandler('<caret>')"),
]);
let feature = EventsFeature::default();
fx.index(&feature);
let (doc, at) = fx.caret("shop/client/main.lua");
let items = feature.completion(&fx.cx(&doc), at, None);
```

## Pull requests

- One change per pull request, with tests for the behaviour it adds or fixes.
- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test --lib` pass; the component
  builds for `wasm32-wasip2` without warnings.
- Comments: none by default. Names carry the meaning; keep a terse comment only where a contract is not obvious
  (a time budget, UTF-16 columns, a host API pitfall).
- User-visible text is English; keep messages short and concrete.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) in English, one concise line:
  `feat(events): …`, `fix(manifest): …`, `docs: …`.
- A behaviour change in the editor is tried in OpenIDEA, not only in unit tests: say how you checked it.

## Updating the natives snapshot

`data/natives/` holds gzip copies of the four databases the plugin reads (their addresses are in
`src/natives/sources.rs`). The [Natives snapshot](.github/workflows/natives.yml) workflow refreshes them every Monday:
when a database changed, it runs the tests and commits the new copies to `main`. To do it by hand:

```sh
scripts/update-natives.sh
cargo test --lib
```

The script only rewrites the files whose content changed.

## OpenIDEA versions

The plugin is built against OpenIDEA's plugin SDK at the revision pinned in `Cargo.toml`.
`scripts/set-openidea-requirement.sh` writes the OpenIDEA it needs into the README, this file and the user guide:
`source` while no OpenIDEA release has the plugin API (OpenIDEA built from the SDK revision in `Cargo.lock`), or the
first release that installs the plugin. The [OpenIDEA compatibility](.github/workflows/openidea-compat.yml) workflow
keeps it true every day: with the latest OpenIDEA release, it runs `openidea-plugin check` on the latest package,
then installs it and runs a command with that release's plugin host (`scripts/openidea-start-check.rs`, since `check`
does not compare interface types). It switches the requirement to the first release that passes, follows the SDK
revision until then, and opens an issue when a release cannot run the plugin.

## Releasing

1. Set the version in `plugin.toml` (with its `change-notes`) and `Cargo.toml`, and move the `[Unreleased]` notes of
   `CHANGELOG.md` under it.
2. Tag the commit `v<version>` and push the tag. The [Release](.github/workflows/release.yml) workflow tests, builds
   and publishes the package file; the [Marketplace](.github/workflows/marketplace.yml) workflow then signs a copy and
   updates the index on the `marketplace` release.

Signing uses the repository secret `PLUGIN_SIGNING_KEY`: the content of the file `openidea-plugin keygen` writes, whose
public key the index lists for the `sensity` publisher. Without the secret, a release goes on without the marketplace.
To publish a release to the marketplace again, run the Marketplace workflow by hand with its tag, or
`SIGNING_KEY="$(cat sensity.key)" scripts/publish-marketplace.sh v<version>` with `openidea-plugin` on your `PATH`.
The index keeps the revocations and the earlier keys it lists; to stop trusting a key, set its `revoked` to `true` in
`index.json` and upload it again.

## License

By contributing, you agree that your contributions are licensed under the [Apache License 2.0](LICENSE).
