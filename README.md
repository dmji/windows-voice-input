# Windows Voice Input

A local, push-to-talk voice input utility for Windows 11.

## Current implementation

- Rust desktop window with recording controls.
- Microphone capture through WASAPI/CPAL.
- Local transcription through an OpenAI-compatible Whisper HTTP endpoint.
- Automatic language detection (the request does not force a language).
- Copy and paste actions, including global hotkeys:
  - `Ctrl+Alt+C`: record, transcribe, copy.
  - `Ctrl+Alt+V`: record, transcribe, paste.
- No automatic Enter key.

## Requirements

- Windows 11 x64.
- Rust stable (MSVC toolchain) to build the desktop application.
- Docker Desktop with NVIDIA GPU support for the CUDA Whisper container.
- A current NVIDIA driver compatible with Docker Desktop GPU support.

## Start Whisper

From the repository root, run:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\start-whisper.ps1
```

The first launch downloads the `large-v3-turbo` model. The API is bound to localhost only. The script creates a local `whisper.env` with a random API key. Keep that file private.

To stop the server:

```powershell
.\scripts\stop-whisper.ps1
```

## Build and run

```powershell
cargo run -p windows-voice-input
```

Run tests:

```powershell
cargo test --workspace --all-targets
```

## Notes

- Whisper language detection is automatic because the client omits the `language` parameter.
- The app reads `WHISPER_API_KEY`, optional `WHISPER_URL`, and optional `WHISPER_BEAM` from `whisper.env` in the current working directory.
- Keep the target application focused before using the paste hotkey.
- The microphone, GPU container, and global hotkeys require an actual Windows machine for end-to-end verification.
- System tray integration and wake-word activation are not implemented yet.
