//! Inkboard for macOS, Windows and Linux. The whole app (library, editor,
//! toolbars, team boards) is `canvas_core`, the same code the Android app
//! runs; this host supplies a window, mouse/pen/touch/keyboard input, text
//! rendering, the clipboard and file dialogs.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod text;

use canvas_core::import::{FileKind, PickerFile};
use canvas_core::ui::app::{App, Phase, Platform, Sample, Shortcut};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

/// Things the app asked for that answer back into it.
enum HostEvent {
    Ink(u32),
    File(i32, PickerFile),
    Import(Vec<String>),
    Toast(String),
    Updates(bool),
    SignedIn(String),
    SignInFailed(String),
    Exit,
}

/// Wakes the event loop from background threads.
#[derive(Debug, Clone, Copy)]
struct Wake;

/// The text field the app is editing, standing in for Android's keyboard.
#[derive(Default)]
struct Editing {
    text: String,
    multiline: bool,
}

struct DesktopPlatform {
    events: Arc<Mutex<Vec<HostEvent>>>,
    editing: Arc<Mutex<Option<Editing>>>,
    proxy: EventLoopProxy<Wake>,
}

impl DesktopPlatform {
    fn post(&self, e: HostEvent) {
        self.events.lock().unwrap().push(e);
        let _ = self.proxy.send_event(Wake);
    }
}

/// Opens `url` in the default browser.
fn open_in_browser(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    r.is_ok()
}

fn pictures_dir() -> PathBuf {
    dirs::picture_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from(".")).join("Inkboard")
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

impl Platform for DesktopPlatform {
    fn show_keyboard(&mut self, text: &str, multiline: bool) {
        *self.editing.lock().unwrap() = Some(Editing { text: text.to_string(), multiline });
    }

    fn hide_keyboard(&mut self) {
        *self.editing.lock().unwrap() = None;
    }

    fn read_ink(&mut self, token: u32, _lang: &str, _data: &[f32]) {
        self.post(HostEvent::Ink(token));
    }

    fn check_updates(&mut self, manual: bool) {
        if manual {
            self.post(HostEvent::Updates(manual));
        }
    }

    fn sign_in_in_browser(&mut self, server: &str) -> bool {
        let events = self.events.clone();
        let proxy = self.proxy.clone();
        canvas_core::ui::cloud::loopback::start(server, None, open_in_browser, move |result| {
            events.lock().unwrap().push(match result {
                Ok(token) => HostEvent::SignedIn(token),
                Err(message) => HostEvent::SignInFailed(message),
            });
            let _ = proxy.send_event(Wake);
        })
    }

    fn open_url(&mut self, url: &str) {
        open_in_browser(url);
    }

    fn copy_text(&mut self, text: &str) {
        if let Ok(mut c) = arboard::Clipboard::new() {
            let _ = c.set_text(text.to_string());
        }
    }

    fn save_png(&mut self, name: &str, png: &[u8]) {
        let dir = pictures_dir();
        let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let path = dir.join(format!("{} {stamp}.png", if safe.trim().is_empty() { "Inkboard" } else { safe.trim() }));
        let result = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, png));
        self.post(HostEvent::Toast(match result {
            Ok(()) => format!("Saved to {}", path.display()),
            Err(e) => format!("Could not save the picture: {e}"),
        }));
    }

    fn pick_boards(&mut self) {
        let Some(files) = rfd::FileDialog::new().set_title("Import boards").add_filter("Inkboard boards", &["json"]).pick_files() else {
            return;
        };
        let texts = files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect();
        self.post(HostEvent::Import(texts));
    }

    fn pick_file(&mut self, token: i32, kind: FileKind) {
        let dialog = rfd::FileDialog::new();
        let dialog = match kind {
            FileKind::Image => dialog.set_title("Insert image").add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif", "bmp"]),
            FileKind::Pdf => dialog.set_title("Insert PDF").add_filter("PDF", &["pdf"]),
            FileKind::Document => dialog.set_title("Insert document").add_filter("Documents", &["svg", "pptx", "docx", "xlsx"]),
        };
        let file = match dialog.pick_file() {
            None => PickerFile::Cancelled,
            Some(_) if kind == FileKind::Pdf => PickerFile::Error("PDF pages can't be imported on this computer yet. Insert them from an Android panel.".into()),
            Some(path) => match std::fs::read(&path) {
                Ok(bytes) if bytes.len() <= 32 << 20 => PickerFile::File {
                    name: path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string(),
                    mime: mime_for(&path).to_string(),
                    bytes,
                },
                Ok(_) => PickerFile::Error("Files must be 32 MB or smaller".into()),
                Err(e) => PickerFile::Error(format!("Could not read the file: {e}")),
            },
        };
        self.post(HostEvent::File(token, file));
    }

    fn exit(&mut self) {
        self.post(HostEvent::Exit);
    }
}

struct Host {
    proxy: EventLoopProxy<Wake>,
    events: Arc<Mutex<Vec<HostEvent>>>,
    editing: Arc<Mutex<Option<Editing>>>,
    app: Option<App>,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    start: Instant,
    mouse: PhysicalPosition<f64>,
    buttons: u8,
    modifiers: ModifiersState,
    size: (u32, u32),
    /// The window buffer holds the previous frame, so only changes are copied.
    shown: bool,
}

impl Host {
    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn sample(&self, id: i32, x: f64, y: f64, stylus: bool) -> Sample {
        Sample { id, x: x as f32, y: y as f32, time: self.now(), size: 0.0, stylus, eraser: false, norm: 0.0 }
    }

    fn redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn deliver(&mut self, event_loop: &ActiveEventLoop) {
        let events: Vec<HostEvent> = std::mem::take(&mut *self.events.lock().unwrap());
        let Some(app) = self.app.as_mut() else { return };
        for e in events {
            match e {
                HostEvent::Ink(token) => {
                    app.ink_result(token, "");
                    app.toast("Handwriting to text needs an Android panel for now");
                }
                HostEvent::File(token, file) => app.file_result(token, file),
                HostEvent::Import(texts) => app.import_boards(texts),
                HostEvent::Toast(text) => app.toast(&text),
                HostEvent::Updates(manual) => app.update_result(None, false, manual),
                HostEvent::SignedIn(token) => app.finish_sign_in(&token),
                HostEvent::SignInFailed(message) => app.sign_in_failed(&message),
                HostEvent::Exit => {
                    self.save_and_exit(event_loop);
                    return;
                }
            }
        }
    }

    fn save_and_exit(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(app) = self.app.as_mut() {
            let (tx, rx) = mpsc::sync_channel(1);
            app.pause(tx);
            // The save runs on the app's disk thread.
            let deadline = Instant::now() + Duration::from_secs(5);
            while rx.try_recv().is_err() && Instant::now() < deadline {
                app.frame(self.start.elapsed().as_millis() as u64);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        event_loop.exit();
    }

    fn paint(&mut self) {
        let now = self.now();
        let (Some(app), Some(surface)) = (self.app.as_mut(), self.surface.as_mut()) else { return };
        let dirty = app.frame(now);
        let (w, h) = self.size;
        if w == 0 || h == 0 {
            return;
        }
        let out = app.output();
        if out.w != w || out.h != h {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else { return };
        let full = !self.shown || buffer.age() != 1;
        let (x0, y0, x1, y1) = match (full, dirty) {
            (true, _) => (0, 0, w as i32, h as i32),
            (false, Some(r)) => (r.x0.max(0), r.y0.max(0), r.x1.min(w as i32), r.y1.min(h as i32)),
            (false, None) => return,
        };
        let data = &out.data;
        for y in y0..y1 {
            let row = y as usize * w as usize;
            for x in x0..x1 {
                let i = (row + x as usize) * 4;
                buffer[row + x as usize] = (data[i] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i + 2] as u32;
            }
        }
        if buffer.present().is_ok() {
            self.shown = true;
        }
    }

    fn schedule(&self, event_loop: &ActiveEventLoop) {
        let Some(app) = &self.app else { return };
        match app.next_deadline() {
            Some(deadline) => {
                let now = self.now();
                if deadline <= now {
                    self.redraw();
                    event_loop.set_control_flow(ControlFlow::Wait);
                } else {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(self.start + Duration::from_millis(deadline)));
                }
            }
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    /// Typing into the field the app is editing.
    fn type_key(&mut self, key: &Key, text: Option<&str>) -> bool {
        let mut editing = self.editing.lock().unwrap();
        let Some(field) = editing.as_mut() else { return false };
        let mut submit = false;
        match key {
            Key::Named(NamedKey::Enter) if field.multiline && self.modifiers.shift_key() => field.text.push('\n'),
            Key::Named(NamedKey::Enter) => submit = true,
            Key::Named(NamedKey::Backspace) => {
                field.text.pop();
            }
            Key::Named(NamedKey::Escape) => {
                drop(editing);
                if let Some(app) = self.app.as_mut() {
                    app.back();
                }
                return true;
            }
            Key::Character(c) if self.modifiers.control_key() || self.modifiers.super_key() => {
                if c.as_str().eq_ignore_ascii_case("v") {
                    if let Ok(pasted) = arboard::Clipboard::new().and_then(|mut cb| cb.get_text()) {
                        field.text.push_str(if field.multiline { &pasted } else { pasted.lines().next().unwrap_or("") });
                    }
                } else {
                    return true;
                }
            }
            _ => match text {
                Some(t) if !t.chars().any(char::is_control) => field.text.push_str(t),
                _ => return true,
            },
        }
        let value = field.text.clone();
        drop(editing);
        if let Some(app) = self.app.as_mut() {
            app.text_input(&value, submit);
        }
        true
    }
}

impl ApplicationHandler<Wake> for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Inkboard")
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .with_min_inner_size(LogicalSize::new(480.0, 400.0));
        let window = Rc::new(event_loop.create_window(attrs).expect("open a window"));
        let context = softbuffer::Context::new(window.clone()).expect("graphics context");
        self.surface = Some(softbuffer::Surface::new(&context, window.clone()).expect("window surface"));
        window.set_ime_allowed(true);

        let data = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("Inkboard");
        let proxy = self.proxy.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = proxy.send_event(Wake);
        });
        let platform = DesktopPlatform { events: self.events.clone(), editing: self.editing.clone(), proxy: self.proxy.clone() };
        let offset = utc_offset_minutes();
        self.app = Some(App::new(Box::new(platform), data.join("boards"), data.join("settings.json"), env!("CARGO_PKG_VERSION").into(), offset, wake));
        self.window = Some(window.clone());
        let size = window.inner_size();
        self.resize(size.width, size.height, window.scale_factor());
        window.request_redraw();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: Wake) {
        self.deliver(event_loop);
        self.redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if self.app.is_none() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => self.save_and_exit(event_loop),
            WindowEvent::Resized(size) => {
                let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
                self.resize(size.width, size.height, scale);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let (w, h) = self.size;
                self.resize(w, h, scale_factor);
            }
            WindowEvent::RedrawRequested => self.paint(),
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse = position;
                let s = self.sample(0, position.x, position.y, false);
                let app = self.app.as_mut().unwrap();
                if self.buttons != 0 {
                    app.touch(Phase::Move, &[s]);
                } else {
                    app.hover(s.x, s.y);
                }
                self.redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                self.app.as_mut().unwrap().hover_exit();
                self.redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let bit = match button {
                    MouseButton::Left => 1,
                    MouseButton::Middle => 2,
                    _ => return,
                };
                let s = self.sample(0, self.mouse.x, self.mouse.y, false);
                let app = self.app.as_mut().unwrap();
                match state {
                    ElementState::Pressed if self.buttons == 0 => {
                        self.buttons |= bit;
                        app.touch(Phase::Down, &[s]);
                    }
                    ElementState::Released if self.buttons & bit != 0 => {
                        self.buttons &= !bit;
                        app.touch(Phase::Up, &[s]);
                    }
                    _ => {}
                }
                self.redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x * 60.0, -y * 60.0),
                    MouseScrollDelta::PixelDelta(p) => (-p.x as f32, -p.y as f32),
                };
                let zoom = self.modifiers.control_key() || self.modifiers.super_key();
                self.app.as_mut().unwrap().wheel(self.mouse.x as f32, self.mouse.y as f32, dx, dy, zoom);
                self.redraw();
            }
            WindowEvent::PinchGesture { delta, .. } => {
                let app = self.app.as_mut().unwrap();
                app.wheel(self.mouse.x as f32, self.mouse.y as f32, 0.0, -(delta as f32) * 400.0, true);
                self.redraw();
            }
            WindowEvent::Touch(t) => {
                let s = self.sample(t.id as i32 + 10, t.location.x, t.location.y, false);
                let phase = match t.phase {
                    TouchPhase::Started => Phase::Down,
                    TouchPhase::Moved => Phase::Move,
                    TouchPhase::Ended => Phase::Up,
                    TouchPhase::Cancelled => Phase::Cancel,
                };
                self.app.as_mut().unwrap().touch(phase, &[s]);
                self.redraw();
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                let typed = self.type_key(&Key::Character(text.as_str().into()), Some(&text));
                if typed {
                    self.redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if self.type_key(&event.logical_key, event.text.as_deref()) {
                    self.redraw();
                    return;
                }
                let command = self.modifiers.control_key() || self.modifiers.super_key();
                let app = self.app.as_mut().unwrap();
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) | Key::Named(NamedKey::BrowserBack) => {
                        app.back();
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        app.key(19);
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        app.key(20);
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        app.key(21);
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        app.key(22);
                    }
                    Key::Named(NamedKey::Enter) => {
                        app.key(66);
                    }
                    Key::Named(NamedKey::Delete | NamedKey::Backspace) => app.shortcut(Shortcut::Delete),
                    Key::Character(c) if command => match c.to_ascii_lowercase().as_str() {
                        "z" if self.modifiers.shift_key() => app.shortcut(Shortcut::Redo),
                        "z" => app.shortcut(Shortcut::Undo),
                        "y" => app.shortcut(Shortcut::Redo),
                        "=" | "+" => app.shortcut(Shortcut::ZoomIn),
                        "-" => app.shortcut(Shortcut::ZoomOut),
                        "0" => app.shortcut(Shortcut::Fit),
                        _ => {}
                    },
                    _ => {}
                }
                self.redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule(event_loop);
    }

    fn new_events(&mut self, _: &ActiveEventLoop, cause: winit::event::StartCause) {
        if matches!(cause, winit::event::StartCause::ResumeTimeReached { .. }) {
            self.redraw();
        }
    }
}

impl Host {
    fn resize(&mut self, w: u32, h: u32, scale: f64) {
        self.size = (w, h);
        self.shown = false;
        if let (Some(surface), Some(nw), Some(nh)) = (self.surface.as_mut(), NonZeroU32::new(w), NonZeroU32::new(h)) {
            let _ = surface.resize(nw, nh);
        }
        // The interface is sized for wall panels; desktops sit closer.
        let ui = std::env::var("INKBOARD_UI_SCALE").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.2);
        if let Some(app) = self.app.as_mut() {
            app.resize(w, h, (scale * ui) as f32);
        }
        self.redraw();
    }
}

/// Minutes east of UTC, for "Today" in the board list.
fn utc_offset_minutes() -> i32 {
    std::env::var("INKBOARD_UTC_OFFSET").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn main() {
    canvas_core::text::set_host(Arc::new(text::DesktopText::new()));
    let event_loop = EventLoop::<Wake>::with_user_event().build().expect("event loop");
    let mut host = Host {
        proxy: event_loop.create_proxy(),
        events: Arc::default(),
        editing: Arc::default(),
        app: None,
        window: None,
        surface: None,
        start: Instant::now(),
        mouse: PhysicalPosition::new(0.0, 0.0),
        buttons: 0,
        modifiers: ModifiersState::empty(),
        size: (0, 0),
        shown: false,
    };
    event_loop.run_app(&mut host).expect("run");
}
