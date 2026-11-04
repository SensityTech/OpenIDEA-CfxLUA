# Security policy

## Supported versions

Security fixes go into the latest release. Please update before reporting.

## Reporting a vulnerability

Please do **not** open a public issue. Report it privately through
[GitHub's private vulnerability reporting](https://github.com/SensityTech/OpenIDEA-CfxLUA/security/advisories/new)
with a description, the affected version and, if possible, a minimal project that reproduces it. You will get an
answer within a few days; once a fix is released, the advisory is published with credit to you unless you prefer
otherwise.

## What the plugin can access

OpenIDEA runs the plugin as a sandboxed WebAssembly component: no file system, network or processes except through
the IDE's API, under the permissions granted at installation (listed in the [README](README.md#permissions)).

- It reads the files of the open projects and shows problems in them.
- It writes only to the focused editor, when you run a command or a tool window action, and to its own storage.
- Its only network requests are natives updates, to `static.cfx.re` and `raw.githubusercontent.com`, and only when
  those optional permissions are granted.
- It runs no programs.

Anything that lets a project, a downloaded database or the tool window page make the plugin go beyond this is a
vulnerability we want to hear about.
