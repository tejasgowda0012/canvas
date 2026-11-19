//! Android owns the window and system APIs; the complete UI runs in Rust.
use super::{log, now_ms, present, string, Window};
use canvas_core::import::{FileKind, PickerFile};
use canvas_core::ui::app::{App, Phase, Platform, Sample};
use jni::objects::{GlobalRef, JByteArray, JClass, JFloatArray, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::{JNIEnv, JavaVM};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

struct AndroidUi {
    vm: JavaVM,
    host: GlobalRef,
    /// The app thread's events, for answers that arrive from other threads.
    events: Sender<Event>,
}
impl AndroidUi {
    fn call(&self, name: &str, sig: &str, args: &[JValue]) {
        if let Ok(mut env) = self.vm.attach_current_thread_as_daemon() {
            if env.call_method(&self.host, name, sig, args).is_err() {
                let _ = env.exception_clear();
                log(6, &format!("Android UI service failed: {name}"));
            }
        }
    }
    fn text_call(&self, name: &str, sig: &str, value: &str, tail: bool, use_tail: bool) {
        if let Ok(mut env) = self.vm.attach_current_thread_as_daemon() {
            let _ = env
                .with_local_frame(8, |env| -> jni::errors::Result<()> {
                    let text = env.new_string(value)?;
                    let mut args = vec![JValue::Object(text.as_ref())];
                    if use_tail {
                        args.push(JValue::Bool(tail as u8));
                    }
                    env.call_method(&self.host, name, sig, &args)?;
                    Ok(())
                })
                .map_err(|_| {
                    let _ = env.exception_clear();
                    log(6, name);
                });
        }
    }
}
impl Platform for AndroidUi {
    fn haptic(&mut self) {
        self.call("haptic", "()V", &[]);
    }
    fn show_keyboard(&mut self, text: &str, multiline: bool) {
        self.text_call(
            "showKeyboard",
            "(Ljava/lang/String;Z)V",
            text,
            multiline,
            true,
        );
    }
    fn hide_keyboard(&mut self) {
        self.call("hideKeyboard", "()V", &[]);
    }
    fn prepare_handwriting(&mut self, lang: &str) {
        self.text_call(
            "prepareHandwriting",
            "(Ljava/lang/String;)V",
            lang,
            false,
            false,
        );
    }
    fn read_ink(&mut self, token: u32, lang: &str, data: &[f32]) {
        if let Ok(mut env) = self.vm.attach_current_thread_as_daemon() {
            let _ = env
                .with_local_frame(8, |env| -> jni::errors::Result<()> {
                    let language = env.new_string(lang)?;
                    let points = env.new_float_array(data.len() as i32)?;
                    env.set_float_array_region(&points, 0, data)?;
                    env.call_method(
                        &self.host,
                        "readInk",
                        "(ILjava/lang/String;[F)V",
                        &[
                            JValue::Int(token as i32),
                            JValue::Object(language.as_ref()),
                            JValue::Object(points.as_ref()),
                        ],
                    )?;
                    Ok(())
                })
                .map_err(|_| {
                    let _ = env.exception_clear();
                    log(6, "readInk failed");
                });
        }
    }
    fn check_updates(&mut self, manual: bool) {
        self.call("checkUpdates", "(Z)V", &[JValue::Bool(manual as u8)]);
    }
    fn install_update(&mut self, url: &str) {
        self.text_call("installUpdate", "(Ljava/lang/String;)V", url, false, false);
    }
    fn set_fast_ink(&mut self, on: bool) {
        crate::fastpresent::set_enabled(on);
    }
    fn set_full_resolution(&mut self, on: bool) {
        self.call("setFullResolution", "(Z)V", &[JValue::Bool(on as u8)]);
    }
    fn copy_text(&mut self, text: &str) {
        self.text_call("copyText", "(Ljava/lang/String;)V", text, false, false);
    }
    fn open_url(&mut self, url: &str) {
        self.text_call("openUrl", "(Ljava/lang/String;)V", url, false, false);
    }
    fn sign_in_in_browser(&mut self, server: &str) -> bool {
        // Google in the phone's browser; it comes back to a port of this app.
        // The page it lands on has a button that returns to Inkboard.
        let back = "intent:#Intent;action=android.intent.action.MAIN;category=android.intent.category.LAUNCHER;package=com.canvasstudio.rust;end".to_string();
        let events = self.events.clone();
        canvas_core::ui::cloud::loopback::start(
            server,
            Some(back),
            |url| {
                self.text_call("openUrl", "(Ljava/lang/String;)V", url, false, false);
                true
            },
            move |result| {
                let _ = events.send(Event::SignIn(result));
            },
        )
    }
    fn save_png(&mut self, name: &str, png: &[u8]) {
        if let Ok(mut env) = self.vm.attach_current_thread_as_daemon() {
            let _ = env
                .with_local_frame(8, |env| -> jni::errors::Result<()> {
                    let title = env.new_string(name)?;
                    let bytes = env.byte_array_from_slice(png)?;
                    env.call_method(
                        &self.host,
                        "savePng",
                        "(Ljava/lang/String;[B)V",
                        &[JValue::Object(title.as_ref()), JValue::Object(bytes.as_ref())],
                    )?;
                    Ok(())
                })
                .map_err(|_| {
                    let _ = env.exception_clear();
                    log(6, "savePng failed");
                });
        }
    }
    fn pick_boards(&mut self) {
        self.call("pickBoards", "()V", &[]);
    }

    fn pick_file(&mut self, token: i32, kind: FileKind) {
        self.call("pickFile", "(II)V", &[JValue::Int(token), JValue::Int(kind.picker_id())]);
    }

    fn exit(&mut self) {
        self.call("finishApp", "()V", &[]);
    }
}

enum Event {
    Surface(Option<Window>, u32, u32, f32, SyncSender<()>),
    Touch(Phase, Vec<Sample>),
    Key(i32),
    Back,
    Text(String, bool),
    Ink(u32, String),
    Update(Option<(String, String)>, bool, bool),
    Toast(String),
    SignIn(Result<String, String>),
    Import(Vec<String>),
    File(i32, PickerFile),
    Pause(SyncSender<()>),
    Wake,
    Quit,
}
struct UiEngine {
    tx: Sender<Event>,
    thread: Mutex<Option<JoinHandle<()>>>,
}
fn engine<'a>(handle: jlong) -> Option<&'a UiEngine> {
    if handle == 0 {
        None
    } else {
        Some(unsafe { &*(handle as *const UiEngine) })
    }
}
fn send(handle: jlong, event: Event) {
    if let Some(e) = engine(handle) {
        let _ = e.tx.send(event);
    }
}

fn drive(mut app: App, rx: Receiver<Event>) {
    let mut window = None;
    let mut deadline = None;
    let mut stats = (0u32, 0f64, 0f64);
    loop {
        let next = match deadline {
            Some(at) => match rx.recv_timeout(Duration::from_millis(
                (at as f64 - now_ms()).max(0.0) as u64 + 1,
            )) {
                Ok(event) => Some(event),
                Err(RecvTimeoutError::Timeout) => None,
                Err(_) => break,
            },
            None => match rx.recv() {
                Ok(event) => Some(event),
                Err(_) => break,
            },
        };
        app.advance_clock(now_ms() as u64);
        let mut next = next;
        let mut quit = false;
        let mut surface_acks = Vec::new();
        let mut latest_sample = None;
        let mut lifted = false;
        while let Some(event) = next {
            match event {
                Event::Surface(w, wid, h, d, ack) => {
                    window = w;
                    if window.is_some() {
                        app.resize(wid, h, d);
                    }
                    surface_acks.push(ack);
                }
                Event::Touch(phase, samples) => {
                    latest_sample = samples.last().map(|s| s.time);
                    lifted |= matches!(phase, Phase::Up | Phase::Cancel);
                    app.touch(phase, &samples);
                }
                Event::Key(key) => {
                    app.key(key);
                }
                Event::Back => app.request_back(),
                Event::Text(text, submit) => app.text_input(&text, submit),
                Event::Ink(token, text) => app.ink_result(token, &text),
                Event::Update(release, failed, manual) => {
                    app.update_result(release, failed, manual)
                }
                Event::Toast(text) => app.toast(&text),
                Event::SignIn(Ok(token)) => app.finish_sign_in(&token),
                Event::SignIn(Err(message)) => app.sign_in_failed(&message),
                Event::Import(files) => app.import_boards(files),
                Event::File(token, file) => app.file_result(token, file),
                Event::Pause(ack) => app.pause(ack),
                Event::Wake => {}
                Event::Quit => quit = true,
            }
            next = rx.try_recv().ok();
        }
        let start = now_ms();
        if let Some(dirty) = app.frame(start as u64) {
            if let Some(window) = &window {
                present(window, app.output(), dirty);
            }
            if let Some(sample) = latest_sample {
                stats.0 += 1;
                stats.1 += now_ms() - sample as f64;
                stats.2 = stats.2.max(now_ms() - start);
            }
        }
        if lifted && stats.0 > 0 {
            log(4,&format!("Rust UI gesture: {} frames; sample-to-present avg {:.1}ms; max frame work {:.1}ms",stats.0,stats.1/stats.0 as f64,stats.2));
            stats = (0, 0.0, 0.0);
        }
        for ack in surface_acks {
            let _ = ack.send(());
        }
        if quit {
            break;
        }
        deadline = app.next_deadline();
    }
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_create(
    mut env: JNIEnv,
    _: JClass,
    host: JObject,
    dir: JString,
    settings: JString,
    version: JString,
    offset: jint,
) -> jlong {
    let Ok(vm) = env.get_java_vm() else { return 0 };
    let Ok(host) = env.new_global_ref(host) else {
        return 0;
    };
    let dir = PathBuf::from(string(&mut env, &dir));
    let settings = PathBuf::from(string(&mut env, &settings));
    let version = string(&mut env, &version);
    let (tx, rx) = mpsc::channel();
    let wake = tx.clone();
    let events = tx.clone();
    let thread = std::thread::Builder::new()
        .name("rust-app".into())
        .spawn(move || {
            let app = App::new(
                Box::new(AndroidUi { vm, host, events }),
                dir,
                settings,
                version,
                offset,
                Arc::new(move || {
                    let _ = wake.send(Event::Wake);
                }),
            );
            drive(app, rx);
        });
    match thread {
        Ok(thread) => Box::into_raw(Box::new(UiEngine {
            tx,
            thread: Mutex::new(Some(thread)),
        })) as jlong,
        Err(_) => 0,
    }
}
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_destroy(
    _: JNIEnv,
    _: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    let e = unsafe { Box::from_raw(handle as *mut UiEngine) };
    let _ = e.tx.send(Event::Quit);
    if let Some(thread) = e.thread.lock().unwrap().take() {
        let _ = thread.join();
    };
}
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_surface(
    env: JNIEnv,
    _: JClass,
    handle: jlong,
    surface: JObject,
    w: jint,
    h: jint,
    dpr: f64,
) {
    let window = if surface.is_null() {
        None
    } else {
        let p = unsafe {
            super::ANativeWindow_fromSurface(env.get_native_interface(), surface.as_raw())
        };
        if p.is_null() {
            None
        } else {
            Some(Window(p))
        }
    };
    let (tx, rx) = mpsc::sync_channel(1);
    send(
        handle,
        Event::Surface(window, w.max(0) as u32, h.max(0) as u32, dpr as f32, tx),
    );
    let _ = rx.recv_timeout(Duration::from_secs(3));
}
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_touch(
    env: JNIEnv,
    _: JClass,
    handle: jlong,
    action: jint,
    base: jlong,
    data: JFloatArray,
    count: jint,
) {
    let n = (count.max(0) as usize * 7).min(env.get_array_length(&data).unwrap_or(0) as usize);
    let mut data_copy = vec![0.0; n];
    if env
        .get_float_array_region(&data, 0, &mut data_copy)
        .is_err()
    {
        return;
    }
    let samples = data_copy
        .chunks_exact(7)
        .filter(|s| s.iter().all(|v| v.is_finite()))
        .map(|s| Sample {
            id: s[0] as i32,
            x: s[1],
            y: s[2],
            time: (base as f64 + s[3] as f64).max(0.0) as u64,
            size: s[4].max(0.0),
            stylus: s[5] != 0.0,
            eraser: s[5] > 1.5,
            norm: s[6],
        })
        .collect();
    let phase = match action {
        0 => Phase::Down,
        1 => Phase::Move,
        2 => Phase::Up,
        _ => Phase::Cancel,
    };
    send(handle, Event::Touch(phase, samples));
}
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_key(
    _: JNIEnv,
    _: JClass,
    h: jlong,
    key: jint,
) {
    send(h, Event::Key(key));
}
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_back(_: JNIEnv, _: JClass, h: jlong) {
    send(h, Event::Back);
}
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_textInput(
    mut e: JNIEnv,
    _: JClass,
    h: jlong,
    t: JString,
    submit: jboolean,
) {
    let t = string(&mut e, &t);
    send(h, Event::Text(t, submit != 0));
}
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_inkResult(
    mut e: JNIEnv,
    _: JClass,
    h: jlong,
    token: jint,
    t: JString,
) {
    let t = string(&mut e, &t);
    send(h, Event::Ink(token as u32, t));
}
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_updateResult(
    mut e: JNIEnv,
    _: JClass,
    h: jlong,
    v: JString,
    url: JString,
    failed: jboolean,
    manual: jboolean,
) {
    let v = string(&mut e, &v);
    let url = string(&mut e, &url);
    send(
        h,
        Event::Update(
            if v.is_empty() { None } else { Some((v, url)) },
            failed != 0,
            manual != 0,
        ),
    );
}
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_toast(
    mut e: JNIEnv,
    _: JClass,
    h: jlong,
    t: JString,
) {
    let t = string(&mut e, &t);
    send(h, Event::Toast(t));
}
/// `files` is a JSON array holding the text of each picked board file.
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_importBoards(
    mut e: JNIEnv,
    _: JClass,
    h: jlong,
    files: JString,
) {
    let files = string(&mut e, &files);
    let files: Vec<String> = serde_json::from_str(&files).unwrap_or_default();
    send(h, Event::Import(files));
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_fileResult(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
    token: jint,
    name: JString,
    mime: JString,
    data: JObjectArray,
    error: JString,
) {
    let file = if !error.is_null() {
        PickerFile::Error(string(&mut env, &error))
    } else if data.is_null() {
        PickerFile::Cancelled
    } else {
        let name = string(&mut env, &name);
        let mime = string(&mut env, &mime);
        let pdf = mime == "application/pdf";
        let mut read = || -> jni::errors::Result<Option<Vec<Vec<u8>>>> {
            let count = env.get_array_length(&data)?;
            if !(1..=if pdf { 80 } else { 1 }).contains(&count) {
                return Ok(None);
            }
            let mut files = Vec::with_capacity(count as usize);
            let mut size_left = 32 << 20;
            for index in 0..count {
                let bytes = JByteArray::from(env.get_object_array_element(&data, index)?);
                let size = env.get_array_length(&bytes)?;
                if size == 0 || size > size_left {
                    return Ok(None);
                }
                size_left -= size;
                files.push(env.convert_byte_array(&bytes)?);
                env.delete_local_ref(bytes)?;
            }
            Ok(Some(files))
        };
        match read() {
            Ok(Some(files)) if pdf => PickerFile::Pdf(files),
            Ok(Some(mut files)) => PickerFile::File { name, mime, bytes: files.remove(0) },
            _ => {
                let _ = env.exception_clear();
                PickerFile::Error("Could not read the selected file".into())
            }
        }
    };
    send(handle, Event::File(token, file));
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_RustApp_pause(_: JNIEnv, _: JClass, h: jlong) {
    let (tx, rx) = mpsc::sync_channel(1);
    send(h, Event::Pause(tx));
    let _ = rx.recv_timeout(Duration::from_secs(3));
}
