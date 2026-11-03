# Windows Voice Input

A local, push-to-talk voice input utility for Windows 11.

## Initial scope

- Rust application core, independent of UI frameworks.
- Local audio capture and transcription are added in later stages.
- No automatic Enter key after inserting recognized text.

## Development

The initial core has no third-party Rust dependencies.

```powershell
cargo test --workspace
```
