#![windows_subsystem = "windows"]

slint::include_modules!();
use std::thread;
use std::sync::{Arc, Mutex};
use std::collections::HashSet;
use std::time::{Instant, Duration};
use rodio::{OutputStream, Sink, buffer::SamplesBuffer};
use native_dialog::FileDialog;
use rdev::{Event, EventType, Key};
use serde::{Serialize, Deserialize};
use std::fs;
use i_slint_backend_winit::WinitWindowAccessor;

#[derive(Serialize, Deserialize, Debug)]
struct AppSettings {
    vol: f32, pitch: f32, start: f32, end: f32, delay: f32, slice: f32, path: String,
}
struct AudioState {
    chunks: Vec<Vec<i16>>,
    index: usize,
    channels: u16,
    sample_rate: u32,
    loudness: f32,
    pitch: f32,
    delay_ms: u64,
    last_played: Instant,
    pressed_keys: HashSet<Key>, 
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    if let Ok(content) = fs::read_to_string("settings.json") {
        if let Ok(s) = serde_json::from_str::<AppSettings>(&content) {
            ui.set_vol_val(s.vol);
            ui.set_pitch_val(s.pitch);
            ui.set_start_val(s.start);
            ui.set_end_val(s.end);
            ui.set_delay_val(s.delay);
            ui.set_slice_val(s.slice);
            ui.set_selected_path(s.path.into());
        }
    }

    // --- SAVE SETTINGS CALLBACK ---
    ui.on_save_settings(|vol, pitch, start, end, delay, slice, path| {
        let s = AppSettings { vol, pitch, start, end, delay, slice, path: path.to_string() };
        if let Ok(json) = serde_json::to_string_pretty(&s) {
            let _ = fs::write("settings.json", json);
            println!("Settings saved to settings.json");
        }
    });
    let audio_state = Arc::new(Mutex::new(AudioState {
        chunks: Vec::new(),
        index: 0,
        channels: 2,
        sample_rate: 44100,
        loudness: 1.0,
        pitch: 1.0,
        delay_ms: 50,
        last_played: Instant::now(),
        pressed_keys: HashSet::new(),
    }));

    // --- KEYBOARD LISTENER ---
    let key_state = Arc::clone(&audio_state);
    thread::spawn(move || {
        let (_stream, stream_handle) = OutputStream::try_default().expect("Audio error");
        
        rdev::listen(move |event: Event| {
            let mut state = key_state.lock().unwrap();
            
            match event.event_type {
                EventType::KeyPress(key) => {

                    if state.pressed_keys.insert(key) {

                        if state.last_played.elapsed() >= Duration::from_millis(state.delay_ms) {
                            if !state.chunks.is_empty() {
                                let buffer = SamplesBuffer::new(
                                    state.channels, 
                                    state.sample_rate, 
                                    state.chunks[state.index].clone()
                                );
                                if let Ok(sink) = Sink::try_new(&stream_handle) {
                                    sink.set_volume(state.loudness);
                                    sink.set_speed(state.pitch);
                                    sink.append(buffer);
                                    sink.detach();
                                    
                                    state.last_played = Instant::now();
                                    state.index = (state.index + 1) % state.chunks.len();
                                }
                            }
                        }
                    }
                }
                EventType::KeyRelease(key) => {

                    state.pressed_keys.remove(&key);
                }
                _ => {}
            }
        }).unwrap();
    });

  
    ui.on_browse_file(|| {
        let path = FileDialog::new()
            .add_filter("WAV Audio", &["wav"])
            .show_open_single_file()
            .unwrap();
        match path {
            Some(p) => p.to_str().unwrap().to_string().into(),
            None => "sounds/click.wav".into(),
        }
    });

    
   let load_state = Arc::clone(&audio_state);
    ui.on_start_loading(move |vol, pitch, clip_start, clip_end, delay, slice_len, file_path| {
        let path_str = file_path.as_str();
        let reader = match hound::WavReader::open(path_str) {
            Ok(r) => r,
            Err(_) => return,
        };
        
        let spec = reader.spec();
        let sr = spec.sample_rate as f32;
        let ch = spec.channels as f32;
        
        let actual_start = clip_start.min(clip_end);
        let actual_end = clip_start.max(clip_end);
        
        let start_sample = (actual_start * sr * ch) as usize;
        let end_sample = (actual_end * sr * ch) as usize;
        let total_samples = end_sample.saturating_sub(start_sample);

        let chunk_size = (slice_len * sr * ch) as usize;

        let all_samples: Vec<i16> = reader.into_samples::<i16>()
            .skip(start_sample)
            .take(total_samples)
            .map(|s| s.unwrap_or(0))
            .collect();

        if all_samples.is_empty() { return; }

        let new_chunks: Vec<Vec<i16>> = all_samples.chunks(chunk_size.max(1))
            .map(|c| c.to_vec())
            .collect();

        let mut state = load_state.lock().unwrap();
        state.chunks = new_chunks;
        state.index = 0;
        state.channels = spec.channels;
        state.sample_rate = spec.sample_rate;
        state.loudness = vol;
        state.pitch = pitch;
        state.delay_ms = delay as u64;
        
        println!("Created {} chunks of {}s length", state.chunks.len(), slice_len);
    }); 

    
    let ui_handle = ui.as_weak();
    ui.on_move_window(move || {
        let ui = ui_handle.unwrap();
        ui.window().with_winit_window(|winit_window| {
            let _ = winit_window.drag_window();
        });
    });

    ui.on_close_window(|| {
        std::process::exit(0);
    });

    
    ui.window().with_winit_window(|winit_window| {
        winit_window.set_decorations(false);
    }); 

    
    ui.run()
}