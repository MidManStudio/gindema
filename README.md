# gindema

Closed-source code/binary protection core **and** gameplay video/image poisoning tool, in one workspace, distributed as public CLIs. Blocks AI-assisted decompilation and reverse engineering, and disrupts AI vision models that would otherwise clone a game from posted gameplay footage.

## Status

Scaffold stage. Crate structure is real; the logic inside each crate is not written yet.

## Workspace layout

Protection core:
- `crates/gindema-common` - shared types/errors (used workspace-wide)
- `crates/gindema-vm` - the custom bytecode VM
- `crates/gindema-obfuscate` - compiler passes
- `crates/gindema-guard` - runtime anti-tamper watchdog
- `crates/gindema-container` - encrypted asset container (built on dixscript)
- `crates/gindema-ptr` - vendored pointer primitives
- `crates/gindema-bits` - vendored bit-level I/O
- `crates/gindema-cli` - the distributable protection-core CLI

Video/image poisoning (placeholder name "mirage", rename before shipping):
- `crates/mirage-core` - perturbation algorithms
- `crates/mirage-ffmpeg` - FFmpeg integration
- `crates/mirage-cli` - the distributable video-poisoning CLI

## License

Proprietary, MidManStudio. See `LICENSE`.
