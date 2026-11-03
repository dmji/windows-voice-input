//! UI-independent application state and messages.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Idle,
    Recording,
    Transcribing,
    Completed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    StartRecording,
    StopRecording,
    ToggleRecording,
    CopyResult,
    PasteResult,
    OpenSettings,
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    RecordingStarted,
    RecordingStopped,
    TranscriptionProgress { percent: u8 },
    TranscriptionCompleted { text: String },
    TranscriptionFailed { message: String },
    MicrophoneLevelChanged { peak: f32 },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppState {
    pub status: Status,
    pub transcript: Option<String>,
    pub error: Option<String>,
    pub transcription_progress: u8,
    pub microphone_level: f32,
}

impl AppState {
    /// Apply an event. Returns false when the event is invalid for the current state.
    pub fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::RecordingStarted
                if matches!(self.status, Status::Idle | Status::Completed | Status::Error) =>
            {
                self.status = Status::Recording;
                self.transcript = None;
                self.error = None;
                self.transcription_progress = 0;
                self.microphone_level = 0.0;
            }
            Event::RecordingStopped if self.status == Status::Recording => {
                self.status = Status::Transcribing;
            }
            Event::TranscriptionProgress { percent }
                if self.status == Status::Transcribing =>
            {
                self.transcription_progress = percent.min(100);
            }
            Event::TranscriptionCompleted { text }
                if self.status == Status::Transcribing =>
            {
                self.status = Status::Completed;
                self.transcript = Some(text);
                self.error = None;
                self.transcription_progress = 100;
            }
            Event::TranscriptionFailed { message }
                if matches!(self.status, Status::Recording | Status::Transcribing) =>
            {
                self.status = Status::Error;
                self.error = Some(message);
            }
            Event::MicrophoneLevelChanged { peak } if self.status == Status::Recording => {
                self.microphone_level = if peak.is_finite() {
                    peak.clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
            _ => return false,
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_to_transcription_to_completed() {
        let mut state = AppState::default();

        assert!(state.apply(Event::RecordingStarted));
        assert_eq!(state.status, Status::Recording);
        assert!(state.apply(Event::RecordingStopped));
        assert_eq!(state.status, Status::Transcribing);
        assert!(state.apply(Event::TranscriptionProgress { percent: 45 }));
        assert_eq!(state.transcription_progress, 45);
        assert!(state.apply(Event::TranscriptionCompleted {
            text: "recognized text".into(),
        }));

        assert_eq!(state.status, Status::Completed);
        assert_eq!(state.transcript.as_deref(), Some("recognized text"));
        assert_eq!(state.transcription_progress, 100);
    }

    #[test]
    fn rejects_events_that_do_not_match_current_state() {
        let mut state = AppState::default();

        assert!(!state.apply(Event::RecordingStopped));
        assert!(!state.apply(Event::TranscriptionCompleted {
            text: "unexpected".into(),
        }));
        assert_eq!(state.status, Status::Idle);
    }

    #[test]
    fn failure_can_be_retried() {
        let mut state = AppState::default();

        assert!(state.apply(Event::RecordingStarted));
        assert!(state.apply(Event::TranscriptionFailed {
            message: "microphone disconnected".into(),
        }));
        assert_eq!(state.status, Status::Error);
        assert!(state.apply(Event::RecordingStarted));
        assert_eq!(state.status, Status::Recording);
        assert_eq!(state.error, None);
    }
}
