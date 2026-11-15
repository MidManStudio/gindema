# pixelflip-cli

## Overview

The pixelflip CLI. Loads an image, applies a `pixelflip-core` pattern,
writes the result. Standalone visual-effects tool, not part of the anti-AI
tooling in this workspace.

## Modules

### `main.rs`

**What it does:** Parses CLI args with `clap` (`--pattern`, `--size`,
`--angle`, `--threshold`, `--blend`, plus positional input/output paths),
maps them onto a `pixelflip_core::Pattern`, and runs `apply()` against the
loaded image.

Usage:

```
pixelflip-cli <input> <output> --pattern diagonal --size 24 --blend 100
```

`--angle` only affects `stripes`. `--threshold` only affects `noise` and
`luminance`. Both are accepted either way and simply ignored by patterns
that don't use them, rather than erroring.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, `workflow_dispatch` only).

## Fixes and Problems

(none yet beyond the shared dependency/MSRV note in `pixelflip-core.md`)
