# User Guide

[简体中文](user-guide.zh-CN.md)

This guide explains how to use `ha-switch` as an everyday local app profile
switcher. It avoids implementation details; see `docs/design.md` when you need
the architecture and safety model.

## Core Ideas

An **app** is a local tool whose state can be managed by `ha-switch`. A build
can include built-in app definitions, and users can add more definitions under
`apps.d/*.yaml`.

A **profile** is one named app state. For example:

- `codex-personal`
- `codex-work`
- `claude-anthropic`
- `claude-proxy`

A **target** is a local place that the app reads or writes, such as a JSON file,
TOML subtree, plain file, Keychain item, or environment fragment.

`ha-switch` never logs in to remote services for you. You log in or configure
the app normally, then use `ha-switch` to save and replay the local state.

## First Run

The current release is a macOS-evidenced stage release. macOS Claude OAuth
import has real local evidence; broader restart checks plus Linux and Windows
real-app evidence are tracked as follow-up work before the project claims full
`docs/design.md` section 13 coverage.

`ha-switch` is distributed as a source-built CLI. The install command compiles
the Rust binary on your machine instead of downloading an unsigned macOS or
Windows binary.

Install Rust first:

```bash
rustup toolchain install 1.95.0
```

If `rustup` is not installed yet, install Rust from <https://rustup.rs> first.

For most npm-based CLI users:

```bash
npm install -g ha-switch
ha-switch --version
```

The first npm install can take a little while because it runs Cargo once and
stores the compiled binary inside the npm package. After that, run
`ha-switch` directly.

For Rust users:

```bash
cargo install ha-switch --locked
ha-switch --version
```

`npx ha-switch --version` can work for a quick trial, but it may compile on
first use and may compile again if npm's cache is cleared. Prefer a global
install for regular use.

For local development from a checkout:

```bash
cargo install --path .
```

Check which apps this build knows about:

```bash
ha-switch apps
```

Find the active `profiles.yaml` path and check local safety diagnostics:

```bash
ha-switch config path
ha-switch doctor
```

`config path` prints the main editable profile registry. `doctor` prints the
ha-switch home directory, the `profiles.yaml` path, permission checks, known
cloud-sync warnings, and app-specific diagnostics.

By default, profiles and captures live under `~/.ha-switch`. This directory can
contain static secrets, OAuth captures, and defensive backups. Keep it out of
cloud-synced folders such as iCloud Drive, Dropbox, OneDrive, and Google Drive;
`doctor` warns when it detects a known sync root.

`HA_SWITCH_HOME` is the preferred override. The legacy `ANY_SWITCH_HOME`
variable is still read as a fallback for existing scripts, but the new name
takes precedence.

To use a separate state directory for testing, choose an absolute path under
your home directory:

```bash
export HA_SWITCH_HOME="$HOME/.ha-switch-test"
```

## Save the Current State

Use `import-current` after the app is already configured or logged in:

```bash
ha-switch import-current <app> personal
```

For OAuth-based app state, close the target app before importing. If the app is
actually closed but process detection reports a false positive, rerun with
`--assume-app-stopped` and confirm the prompt. In scripts and CI, add `--yes`
for that escape hatch.

For built-in Claude OAuth, a typical first capture is:

```bash
ha-switch import-current claude personal --kind oauth_capture
```

Use the process-probe escape hatch only for false positives. OAuth tokens can
rotate while the app is running, so importing live state from a running app can
save an incomplete or stale capture.

For built-in Codex, a typical first capture is:

```bash
ha-switch import-current codex personal
```

## Add a Static Profile

Use `add` when the desired state is made from explicit fields, such as an API
key, endpoint, provider, model, or environment value. The available fields come
from the selected app definition and profile kind.

```bash
ha-switch add <app> work --kind <kind> --field key=value
```

For built-in Codex API-key state:

```bash
ha-switch add codex openai --kind file_template \
  --secret-field api_key=@prompt \
  --field model=gpt-5-codex \
  --field model_provider=openai
```

For Claude-style environment injection:

```bash
ha-switch add claude proxy \
  --kind env_injection \
  --field base_url=https://example.test/api \
  --field models.default=example-model \
  --secret-field auth_token=@env:ANTHROPIC_AUTH_TOKEN
```

Secret fields can be read from a masked interactive prompt, stdin, an
environment variable, or a local file:

```bash
--secret-field api_key=@prompt
--secret-field api_key=@stdin
--secret-field api_key=@env:OPENAI_API_KEY
--secret-field api_key=@file:~/secrets/openai-api-key
```

Use `@prompt` for normal interactive setup. Use `@env:NAME`, `@stdin`, or
`@file:PATH` when scripting. Avoid placing secret values directly in shell
commands.

## Migrate Codex History

Codex profile switching manages only Codex `auth.json` and selected
`config.toml` fields. Conversation history stays shared under `CODEX_HOME` and
is not captured, backed up, restored, or deleted by profile switching.

Preview and merge an old Codex home into the current one:

```bash
ha-switch codex-history migrate --from /path/to/old-codex-home --dry-run
ha-switch codex-history migrate --from /path/to/old-codex-home --yes
```

The migration appends missing `history.jsonl` and `session_index.jsonl` lines,
copies missing `sessions/` files, skips identical files, and stops on
conflicting session files without overwriting them.

## Switch Profiles

Preview first:

```bash
ha-switch use <profile-id> --dry-run
```

Apply the profile:

```bash
ha-switch use <profile-id>
```

In an interactive terminal, confirm the write by typing `yes` when prompted.
Use `--yes` for scripts or CI where no terminal prompt is available.

For dynamic OAuth profiles, `use` first writes back the currently active
profile's latest live capture, but only if the live identity still matches the
active profile. This prevents credentials for one account from being saved into
another account's profile.

## Check Current State

Use `status` for a quick comparison:

```bash
ha-switch status <app>
```

Use `doctor` for more detail:

```bash
ha-switch doctor <app>
```

These commands redact secret values.

## Understand Safety Flags

`--yes` confirms high-risk actions non-interactively, such as `use`,
`restore-target`, `remove`, or `--assume-app-stopped`. In an interactive
terminal you may omit `--yes` and type `yes` at the prompt instead. Neither
confirmation path skips safety checks such as identity checks, backups, locks,
path validation, or secret redaction. `add` and ordinary `import-current` do not
accept `--yes` because they create or capture state instead of overwriting the
target app. `import-current --yes` is valid only with `--assume-app-stopped`.

`--allow-running` is only for static, non-OAuth writes where you intentionally
accept editing local files while the app is running.

`--assume-app-stopped` is only for process-sensitive operations when the app is
actually stopped but process detection is wrong. OAuth import, writeback, and
restore flows use this flag instead of `--allow-running`. Confirm with `--yes`
in scripts, or type `yes` at the interactive prompt. If no matching process was
detected, remove the flag and rerun the command.

## Handle Common Errors

### DriftBeforeWriteback

The live app identity no longer matches the profile that `ha-switch` currently
considers active. The switch is blocked so the wrong live state is not written
back to the old profile.

Inspect the drift:

```bash
ha-switch status <app>
ha-switch doctor <app>
```

If the live state is a useful new profile:

```bash
ha-switch import-current <app> <new-name>
```

If you want to discard the live state and restore a saved profile:

```bash
ha-switch detach <app>
ha-switch use <profile-id>
```

### IdentityMissing

The current app state does not contain the identity fields required by the app
definition. Make sure the app is logged in or configured, then run:

```bash
ha-switch doctor <app>
```

If the app is not using OAuth, retry the import with the right `--kind`, or use
`add` for static profiles.

### TargetMissing

The app has no complete importable state for the selected kind. Run:

```bash
ha-switch doctor <app>
```

For OAuth profiles, check the `definition_capture_source` rows. They show
whether the current platform credential source, such as a Keychain entry or
credentials file, is `exists`, `missing`, or `warning` because existence could
not be confirmed. If the row includes `hint:`, follow that source-specific next
step first. File-source hints usually mean checking the app config directory;
for macOS Keychain warnings, run `doctor` from a local desktop terminal and use
`security find-generic-password` only without `-w` unless you intentionally need
to reveal the credential.

### ImportAmbiguous

More than one import rule matches the current app state. Choose the intended
kind explicitly:

```bash
ha-switch import-current <app> <name> --kind <kind>
```

Or clean up the app's live config so only one state remains.

### AppRunning

Close the target app and retry. For OAuth or process-sensitive operations, use
`--assume-app-stopped` only when process detection is a false positive. Add
`--yes` in non-interactive runs, or type `yes` at the prompt in a terminal.

## Backups and Restore

Before writing managed targets, `ha-switch` creates backups.

List backups:

```bash
ha-switch backup list
```

Restore an app from a backup:

```bash
ha-switch restore-target <app> <backup-id>
```

`restore-target` restores live app state from the backup but does not mark a
profile active. Run `ha-switch status <app>` afterwards to inspect whether the
restored state matches the active profile. Confirm by typing `yes` in an
interactive terminal, or add `--yes` in scripts and CI. For OAuth or
process-sensitive targets, restore follows the same stop-app rule as switching.

Remove a saved profile when you no longer need it:

```bash
ha-switch remove <profile-id>
```

`remove` deletes the profile and its ha-switch capture files. It does not clear
or restore the target app's current live state. Confirm by typing `yes` in an
interactive terminal, or add `--yes` in scripts and CI.

## Edit Profiles

Open a saved profile in your editor:

```bash
ha-switch edit <profile-id>
```

`ha-switch` uses `$VISUAL`, then `$EDITOR`, then the platform default editor
for the build target. It validates the edited profile before saving it.

## Add More Apps

User app definitions live under:

```text
~/.ha-switch/apps.d/*.yaml
```

Use:

```bash
ha-switch apps validate <path>
ha-switch apps show <app>
ha-switch apps export <app> --source system
ha-switch apps export <app> --source resolved
ha-switch apps export <app> --as override --output ~/.ha-switch/overrides.d/<app>.yaml
```

Definitions should describe local state declaratively and reuse trusted handlers
instead of requiring app-specific code in the core CLI.

`--source system` exports the built-in definition bundled in the binary.
`--source resolved` exports the definition after user definitions and overrides
are applied. `--as override` writes a narrow override starting point instead of
a full replacement definition.
