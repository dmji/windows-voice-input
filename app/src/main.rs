use std::{
    fs,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

use arboard::Clipboard;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, SizedSample, Stream,
};
use eframe::egui;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use hound::{SampleFormat as WavSampleFormat, WavSpec, WavWriter};
use reqwest::blocking::{multipart, Client};
use serde::Deserialize;

#[derive(Debug)]
enum WorkerCommand {
    Start,
    Stop,
}

#[derive(Debug)]
enum WorkerEvent {
    RecordingStarted,
    Transcribing,
    Completed(String),
    Failed(String),
}

trait ToF32 {
    fn to_f32(self) -> f32;
}

impl ToF32 for f32 {
    fn to_f32(self) -> f32 { self }
}
impl ToF32 for i16 {
    fn to_f32(self) -> f32 { self as f32 / i16::MAX as f32 }
}
impl ToF32 for u16 {
    fn to_f32(self) -> f32 { (self as f32 - 32768.0) / 32768.0 }
}

struct AudioRecorder {
    stream: Stream,
    samples: Arc<Mutex<Vec<i16>>>,
    sample_rate: u32,
}

impl AudioRecorder {
    fn start() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("No default microphone found")?;
        let supported = device.default_input_config().map_err(|e| format!("Microphone config: {e}"))?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let config = supported.config();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let err_samples = Arc::clone(&samples);

        let stream = match supported.sample_format() {
            SampleFormat::F32 => Self::build::<f32>(&device, &config, channels, Arc::clone(&samples))?,
            SampleFormat::I16 => Self::build::<i16>(&device, &config, channels, Arc::clone(&samples))?,
            SampleFormat::U16 => Self::build::<u16>(&device, &config, channels, Arc::clone(&samples))?,
            format => return Err(format!("Unsupported microphone sample format: {format:?}")),
        };
        let _ = err_samples;
        stream.play().map_err(|e| format!("Cannot start microphone: {e}"))?;
        Ok(Self { stream, samples, sample_rate })
    }

    fn build<T: SizedSample + ToF32>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        channels: usize,
        samples: Arc<Mutex<Vec<i16>>>,
    ) -> Result<Stream, String> {
        device.build_input_stream(
            config,
            move |data: &[T], _| {
                if let Ok(mut output) = samples.lock() {
                    for frame in data.chunks(channels.max(1)) {
                        if frame.is_empty() { continue; }
                        let mono = frame.iter().map(|sample| sample.to_f32()).sum::<f32>() / frame.len() as f32;
                        output.push((mono.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                    }
                }
            },
            |error| eprintln!("Audio stream error: {error}"),
            None,
        ).map_err(|e| format!("Cannot open microphone stream: {e}"))
    }

    fn finish(self) -> Result<Vec<u8>, String> {
        drop(self.stream);
        let samples = self.samples.lock().map_err(|_| "Audio buffer lock failed")?;
        if samples.is_empty() { return Err("No audio was recorded".into()); }

        let path: PathBuf = std::env::temp_dir().join(format!("windows-voice-input-{}.wav", std::process::id()));
        let spec = WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: WavSampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).map_err(|e| format!("Cannot create WAV: {e}"))?;
        for sample in samples.iter() {
            writer.write_sample(*sample).map_err(|e| format!("Cannot write WAV: {e}"))?;
        }
        writer.finalize().map_err(|e| format!("Cannot finalize WAV: {e}"))?;
        drop(samples);
        let bytes = fs::read(&path).map_err(|e| format!("Cannot read WAV: {e}"))?;
        let _ = fs::remove_file(path);
        Ok(bytes)
    }
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

fn read_setting(name: &str) -> Option<String> {
    let path = std::env::current_dir().ok()?.join("whisper.env");
    let content = fs::read_to_string(path).ok()?;
    content.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == name).then(|| value.trim().trim_matches('"').to_string())
    })
}

fn transcribe(wav: Vec<u8>) -> Result<String, String> {
    let key = read_setting("WHISPER_API_KEY").ok_or("whisper.env is missing WHISPER_API_KEY")?;
    let endpoint = read_setting("WHISPER_URL").unwrap_or_else(|| "http://127.0.0.1:9000/v1/audio/transcriptions".into());
    let beam = read_setting("WHISPER_BEAM").unwrap_or_else(|| "5".into());
    let part = multipart::Part::bytes(wav)
        .file_name("speech.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = multipart::Form::new()
        .text("model", "whisper-1")
        .text("response_format", "json")
        .text("beam_size", beam)
        .part("file", part);

    let response = Client::builder()
        .timeout(Duration::from_secs(300))
        .build().map_err(|e| e.to_string())?
        .post(endpoint)
        .bearer_auth(key)
        .multipart(form)
        .send().map_err(|e| format!("Whisper request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!("Whisper returned {status}: {body}"));
    }
    let result: TranscriptionResponse = response.json().map_err(|e| format!("Invalid Whisper response: {e}"))?;
    Ok(result.text.trim().to_string())
}

fn worker(rx: mpsc::Receiver<WorkerCommand>, tx: mpsc::Sender<WorkerEvent>) {
    let mut recorder: Option<AudioRecorder> = None;
    while let Ok(command) = rx.recv() {
        match command {
            WorkerCommand::Start => {
                if recorder.is_some() { continue; }
                match AudioRecorder::start() {
                    Ok(audio) => {
                        recorder = Some(audio);
                        let _ = tx.send(WorkerEvent::RecordingStarted);
                    }
                    Err(error) => { let _ = tx.send(WorkerEvent::Failed(error)); }
                }
            }
            WorkerCommand::Stop => {
                let Some(audio) = recorder.take() else { continue };
                let _ = tx.send(WorkerEvent::Transcribing);
                let result = audio.finish().and_then(transcribe);
                match result {
                    Ok(text) if !text.is_empty() => { let _ = tx.send(WorkerEvent::Completed(text)); }
                    Ok(_) => { let _ = tx.send(WorkerEvent::Failed("Whisper returned an empty transcript".into())); }
                    Err(error) => { let _ = tx.send(WorkerEvent::Failed(error)); }
                }
            }
        }
    }
}

fn copy_text(text: &str) -> Result<(), String> {
    Clipboard::new().map_err(|e| e.to_string())?
        .set_text(text.to_string()).map_err(|e| e.to_string())
}

fn paste_text(text: &str) -> Result<(), String> {
    copy_text(text)?;
    thread::sleep(Duration::from_millis(120));
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("Keyboard init: {e}"))?;
    enigo.key(Key::Control, Direction::Press).map_err(|e| e.to_string())?;
    enigo.key(Key::Unicode('v'), Direction::Click).map_err(|e| e.to_string())?;
    enigo.key(Key::Control, Direction::Release).map_err(|e| e.to_string())
}

struct VoiceApp {
    commands: mpsc::Sender<WorkerCommand>,
    events: mpsc::Receiver<WorkerEvent>,
    hotkeys: GlobalHotKeyManager,
    status: String,
    transcript: String,
    error: Option<String>,
    recording: bool,
    busy: bool,
}

impl VoiceApp {
    fn new() -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        thread::Builder::new().name("voice-worker".into())
            .spawn(move || worker(command_rx, event_tx))
            .expect("cannot start voice worker");

        let hotkeys = GlobalHotKeyManager::new().expect("cannot initialize global hotkeys");
        let copy_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyC);
        let paste_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV);
        hotkeys.register(copy_hotkey).expect("cannot register Ctrl+Alt+C");
        hotkeys.register(paste_hotkey).expect("cannot register Ctrl+Alt+V");

        Self {
            commands: command_tx,
            events: event_rx,
            hotkeys,
            status: "Готово".into(),
            transcript: String::new(),
            error: None,
            recording: false,
            busy: false,
        }
    }

    fn start(&mut self) {
        if self.recording || self.busy { return; }
        if self.commands.send(WorkerCommand::Start).is_err() {
            self.error = Some("Voice worker stopped".into());
        }
    }

    fn stop(&mut self) {
        if !self.recording { return; }
        let _ = self.commands.send(WorkerCommand::Stop);
        self.recording = false;
        self.busy = true;
        self.status = "Распознавание…".into();
    }

    fn handle_hotkeys(&mut self) {
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state() != HotKeyState::Pressed { continue; }
            if event.id() == HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyC).id() {
                if self.recording { self.stop(); } else { self.start(); }
                self.copy_after_transcription = true;
            } else if event.id() == HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV).id() {
                if self.recording { self.stop(); } else { self.start(); }
                self.paste_after_transcription = true;
            }
        }
    }

    fn process_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                WorkerEvent::RecordingStarted => {
                    self.recording = true;
                    self.busy = false;
                    self.status = "Идёт запись…".into();
                    self.error = None;
                }
                WorkerEvent::Transcribing => {
                    self.recording = false;
                    self.busy = true;
                    self.status = "Распознавание…".into();
                }
                WorkerEvent::Completed(text) => {
                    self.transcript = text;
                    self.recording = false;
                    self.busy = false;
                    self.status = "Готово".into();
                    if self.copy_after_transcription {
                        if let Err(error) = copy_text(&self.transcript) { self.error = Some(error); }
                    }
                    if self.paste_after_transcription {
                        if let Err(error) = paste_text(&self.transcript) { self.error = Some(error); }
                    }
                    self.copy_after_transcription = false;
                    self.paste_after_transcription = false;
                }
                WorkerEvent::Failed(error) => {
                    self.recording = false;
                    self.busy = false;
                    self.status = "Ошибка".into();
                    self.error = Some(error);
                    self.copy_after_transcription = false;
                    self.paste_after_transcription = false;
                }
            }
        }
    }

    copy_after_transcription: bool,
    paste_after_transcription: bool,
}

impl Default for VoiceApp {
    fn default() -> Self { Self::new() }
}

impl eframe::App for VoiceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_hotkeys();
        self.process_events();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Windows Voice Input");
            ui.label(format!("Статус: {}", self.status));
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.recording && !self.busy, egui::Button::new("Начать запись")).clicked() {
                    self.start();
                }
                if ui.add_enabled(self.recording, egui::Button::new("Остановить")).clicked() {
                    self.stop();
                }
            });
            ui.separator();
            ui.label("Распознанный текст");
            ui.add(egui::TextEdit::multiline(&mut self.transcript).desired_rows(7).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.transcript.is_empty(), egui::Button::new("Копировать")).clicked() {
                    if let Err(error) = copy_text(&self.transcript) { self.error = Some(error); }
                }
                if ui.add_enabled(!self.transcript.is_empty(), egui::Button::new("Вставить в активное поле")).clicked() {
                    if let Err(error) = paste_text(&self.transcript) { self.error = Some(error); }
                }
            });
            ui.separator();
            ui.label("Ctrl+Alt+C — запись и копирование");
            ui.label("Ctrl+Alt+V — запись и вставка");
            ui.label("Enter автоматически не отправляется.");
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::RED, error);
            }
        });
        ctx.request_repaint_after(Duration::from_millis(30));
        let _ = &self.hotkeys;
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([440.0, 430.0])
            .with_min_inner_size([360.0, 360.0])
            .with_title("Windows Voice Input"),
        ..Default::default()
    };
    eframe::run_native(
        "Windows Voice Input",
        options,
        Box::new(|_cc| Ok(Box::new(VoiceApp::new()))),
    )
}
