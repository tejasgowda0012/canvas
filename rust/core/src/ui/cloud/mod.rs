//! Team boards on the Inkboard server (`cloud/` in the repository, on
//! Cloudflare): Google Workspace sign-in, the list of boards a person can
//! open, sharing, and live editing with everyone else on a board.

pub mod http;
#[cfg(not(target_arch = "wasm32"))]
pub mod loopback;
pub mod replica;
#[cfg(not(target_arch = "wasm32"))]
pub mod room;
#[cfg(target_arch = "wasm32")]
#[path = "room_web.rs"]
pub mod room;

use crate::import::ImageAsset;
use crate::shape::Shape;
use http::{api, ApiError};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::future::Future;
use std::sync::Arc;

/// The server built into release builds. Settings can point elsewhere.
pub const DEFAULT_SERVER: &str = "https://whiteboard.kenpathlabs.com";

#[derive(Clone, Debug, PartialEq, Default)]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
    pub color: u32,
}

impl User {
    pub fn from_json(v: &Value) -> Option<User> {
        Some(User {
            id: v["id"].as_str()?.to_string(),
            email: v["email"].as_str().unwrap_or_default().to_string(),
            name: v["name"].as_str().unwrap_or_default().to_string(),
            color: parse_color(v["color"].as_str().unwrap_or_default()),
        })
    }

    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "email": self.email, "name": self.name, "color": format!("#{:06x}", self.color & 0xffffff) })
    }

    /// Up to two initials for an avatar.
    pub fn initials(&self) -> String {
        initials(if self.name.is_empty() { &self.email } else { &self.name })
    }

    pub fn first_name(&self) -> &str {
        let name = if self.name.is_empty() { self.email.split('@').next().unwrap_or("") } else { &self.name };
        name.split_whitespace().next().unwrap_or(name)
    }
}

pub fn initials(name: &str) -> String {
    let mut out: String = name.split(|c: char| c.is_whitespace() || c == '.' || c == '@').filter(|w| !w.is_empty()).take(2)
        .filter_map(|w| w.chars().next()).flat_map(char::to_uppercase).collect();
    if out.is_empty() {
        out.push('?');
    }
    out
}

/// "#rrggbb" to an opaque ARGB colour.
pub fn parse_color(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches('#'), 16).map_or(0xff4465e9, |c| 0xff000000 | (c & 0xffffff))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Editor,
    Viewer,
}

impl Role {
    pub fn parse(s: &str) -> Role {
        match s {
            "owner" => Role::Owner,
            "editor" => Role::Editor,
            _ => Role::Viewer,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Editor => "editor",
            Role::Viewer => "viewer",
        }
    }

    pub fn can_edit(self) -> bool {
        self != Role::Viewer
    }
}

/// A board on the server, as the board list shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudBoard {
    pub id: String,
    pub title: String,
    pub role: Role,
    /// "none", "view" or "edit".
    pub org_access: String,
    pub owner_name: String,
    pub owner_email: String,
    /// Milliseconds since the Unix epoch.
    pub updated_at: i64,
    /// People invited besides the owner.
    pub members: u32,
}

impl CloudBoard {
    pub fn from_json(v: &Value) -> Option<CloudBoard> {
        Some(CloudBoard {
            id: v["id"].as_str()?.to_string(),
            title: v["title"].as_str().unwrap_or("Untitled board").to_string(),
            role: Role::parse(v["role"].as_str().unwrap_or_default()),
            org_access: v["orgAccess"].as_str().unwrap_or("none").to_string(),
            owner_name: v["owner"]["name"].as_str().unwrap_or_default().to_string(),
            owner_email: v["owner"]["email"].as_str().unwrap_or_default().to_string(),
            updated_at: v["updatedAt"].as_i64().unwrap_or_default(),
            members: v["members"].as_u64().unwrap_or_default() as u32,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub email: String,
    pub name: String,
    pub role: Role,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Members {
    pub org_access: String,
    pub owner: Option<Member>,
    pub members: Vec<Member>,
}

impl Members {
    pub fn from_json(v: &Value) -> Members {
        let member = |m: &Value| Member {
            email: m["email"].as_str().unwrap_or_default().to_string(),
            name: m["name"].as_str().unwrap_or_default().to_string(),
            role: Role::parse(m["role"].as_str().unwrap_or_default()),
        };
        Members {
            org_access: v["orgAccess"].as_str().unwrap_or("none").to_string(),
            owner: v["owner"].as_object().map(|_| member(&v["owner"])),
            members: v["members"].as_array().into_iter().flatten().map(member).collect(),
        }
    }
}

/// Signed-in state, stored with the app's settings.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Account {
    pub server: String,
    pub token: String,
    pub user: User,
}

/// Results of server requests, delivered to the app thread.
pub enum Reply {
    SignIn(SignInStep),
    Boards(Result<Vec<CloudBoard>, ApiError>),
    Created(Result<CloudBoard, ApiError>),
    Members(String, Result<Members, ApiError>),
    /// A board changed (renamed, access changed), or the error.
    Board(String, Result<CloudBoard, ApiError>),
    Deleted(String, Result<(), ApiError>),
}

pub enum SignInStep {
    /// Show this code; the person enters it at `url`.
    Code { code: String, url: String },
    Done(Account),
    Failed(String),
}

/// `Send` where requests run on their own threads; anything in a browser,
/// where they run on the page's event loop.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

/// Runs `work` in the background: on a short-lived thread, or on the page's
/// event loop in a browser.
pub fn run(work: impl Future<Output = ()> + MaybeSend + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = std::thread::Builder::new().name("cloud-request".into()).spawn(move || block_on(work));
    }
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(work);
}

/// Native requests block inside their future, so a future here is only ever
/// waiting on [`sleep`], which also blocks; polling until ready is enough.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn block_on<F: Future>(work: F) -> F::Output {
    let mut work = std::pin::pin!(work);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(out) = work.as_mut().poll(&mut cx) {
            return out;
        }
        std::thread::yield_now();
    }
}

/// Waits `ms` milliseconds.
pub async fn sleep(ms: u32) {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    #[cfg(target_arch = "wasm32")]
    {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(w) = web_sys::window() {
                let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
}

/// Runs a request in the background and posts the result.
pub fn spawn(tx: &Sender<Reply>, wake: &Arc<dyn Fn() + Send + Sync>, job: impl Future<Output = Reply> + MaybeSend + 'static) {
    let (tx, wake) = (tx.clone(), wake.clone());
    run(async move {
        let reply = job.await;
        if tx.send(reply).is_ok() {
            wake();
        }
    });
}

/// Starts Google sign-in. Steps arrive as [`Reply::SignIn`] until done,
/// failed, or `cancel` is set.
pub fn sign_in(server: String, device: String, cancel: Arc<AtomicBool>, tx: Sender<Reply>, wake: Arc<dyn Fn() + Send + Sync>) {
    run(async move {
        let send = |step: SignInStep| {
            let _ = tx.send(Reply::SignIn(step));
            wake();
        };
        let start = match api(&server, "POST", "/api/auth/device", None, Some(&json!({ "device": device }))).await {
            Ok(v) => v,
            Err(e) => return send(SignInStep::Failed(e.message)),
        };
        let handle = start["handle"].as_str().unwrap_or_default().to_string();
        send(SignInStep::Code {
            code: start["userCode"].as_str().unwrap_or_default().to_string(),
            url: start["verificationUrl"].as_str().unwrap_or("https://www.google.com/device").to_string(),
        });
        let mut interval = start["interval"].as_u64().unwrap_or(5).max(2);
        let deadline = crate::sys::now_ms() + 1000 * start["expiresIn"].as_i64().unwrap_or(1800);
        let mut failures = 0;
        while crate::sys::now_ms() < deadline {
            for _ in 0..interval * 10 {
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
                sleep(100).await;
            }
            match api(&server, "POST", "/api/auth/device/poll", None, Some(&json!({ "handle": handle }))).await {
                Ok(v) if v["token"].is_string() => {
                    let user = User::from_json(&v["user"]).unwrap_or_default();
                    let token = v["token"].as_str().unwrap_or_default().to_string();
                    return send(SignInStep::Done(Account { server, token, user }));
                }
                Ok(v) => interval = v["interval"].as_u64().unwrap_or(interval).max(2),
                Err(e) if e.status == 0 && failures < 5 => failures += 1,
                Err(e) => return send(SignInStep::Failed(e.message)),
            }
        }
        send(SignInStep::Failed("The sign-in code expired. Try again.".into()));
    });
}

pub async fn list_boards(a: &Account) -> Result<Vec<CloudBoard>, ApiError> {
    let v = api(&a.server, "GET", "/api/boards", Some(&a.token), None).await?;
    Ok(v["boards"].as_array().into_iter().flatten().filter_map(CloudBoard::from_json).collect())
}

pub async fn create_board(a: &Account, title: &str) -> Result<CloudBoard, ApiError> {
    let v = api(&a.server, "POST", "/api/boards", Some(&a.token), Some(&json!({ "title": title }))).await?;
    CloudBoard::from_json(&v).ok_or_else(|| ApiError::offline("Bad response"))
}

pub async fn update_board(a: &Account, id: &str, change: Value) -> Result<CloudBoard, ApiError> {
    let v = api(&a.server, "PATCH", &format!("/api/boards/{id}"), Some(&a.token), Some(&change)).await?;
    CloudBoard::from_json(&v).ok_or_else(|| ApiError::offline("Bad response"))
}

pub async fn delete_board(a: &Account, id: &str) -> Result<(), ApiError> {
    api(&a.server, "DELETE", &format!("/api/boards/{id}"), Some(&a.token), None).await.map(|_| ())
}

pub async fn members(a: &Account, id: &str) -> Result<Members, ApiError> {
    api(&a.server, "GET", &format!("/api/boards/{id}/members"), Some(&a.token), None).await.map(|v| Members::from_json(&v))
}

pub async fn add_member(a: &Account, id: &str, email: &str, role: Role) -> Result<Members, ApiError> {
    api(&a.server, "PUT", &format!("/api/boards/{id}/members"), Some(&a.token), Some(&json!({ "email": email, "role": role.name() })))
        .await
        .map(|v| Members::from_json(&v))
}

pub async fn remove_member(a: &Account, id: &str, email: &str) -> Result<Members, ApiError> {
    let email: String = email.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    api(&a.server, "DELETE", &format!("/api/boards/{id}/members/{email}"), Some(&a.token), None).await.map(|v| Members::from_json(&v))
}

pub async fn sign_out(a: &Account) {
    let _ = api(&a.server, "POST", "/api/auth/logout", Some(&a.token), None).await;
}

// ------------------------------------------------------------- records

/// The record holding the order of a board's pages.
pub const PAGES_ID: &str = "~pages";

/// What a record holds.
pub enum Record {
    Shape { page: String, shape: Value },
    Pages(Vec<String>),
}

pub fn parse_record(v: &Value) -> Option<Record> {
    if v["id"] == PAGES_ID {
        return Some(Record::Pages(v["pages"].as_array()?.iter().filter_map(|p| p.as_str().map(str::to_string)).collect()));
    }
    Some(Record::Shape { page: v["page"].as_str()?.to_string(), shape: v.get("shape")?.clone() })
}

pub fn record_id(page: &str, shape: &str) -> String {
    format!("{page}/{shape}")
}

/// Turns pages of shapes into records, reusing the JSON of shapes that have
/// not changed since the last call (shapes are replaced, never mutated).
#[derive(Default)]
pub struct RecordCache {
    by_id: HashMap<String, (usize, Value)>,
}

impl RecordCache {
    pub(crate) fn records<'a>(&mut self, pages: impl Iterator<Item = (&'a str, &'a [Arc<Shape>])>) -> (HashMap<String, Value>, Vec<Arc<ImageAsset>>) {
        let mut out = HashMap::new();
        let mut images = Vec::new();
        let mut order = Vec::new();
        let mut next = HashMap::new();
        for (page, shapes) in pages {
            order.push(Value::String(page.to_string()));
            for shape in shapes {
                let id = record_id(page, &shape.id);
                let ptr = Arc::as_ptr(shape) as usize;
                let value = match self.by_id.remove(&id) {
                    Some((p, v)) if p == ptr => v,
                    _ => {
                        let reference = shape.image.as_ref().map(|image| image.hash_name());
                        json!({ "id": id, "page": page, "shape": shape.to_json_with_image_ref(reference) })
                    }
                };
                if let Some(image) = &shape.image {
                    images.push(image.clone());
                }
                next.insert(id.clone(), (ptr, value.clone()));
                out.insert(id, value);
            }
        }
        out.insert(PAGES_ID.to_string(), json!({ "id": PAGES_ID, "pages": order }));
        self.by_id = next;
        (out, images)
    }
}

/// Images that records in a batch refer to.
pub(crate) fn images_for(put: &[Value], all: &[Arc<ImageAsset>]) -> Vec<Arc<ImageAsset>> {
    let wanted: std::collections::HashSet<&str> = put.iter().filter_map(|r| r["shape"]["imageRef"].as_str()).collect();
    let mut seen = std::collections::HashSet::new();
    all.iter().filter(|i| wanted.contains(i.hash_name().as_str()) && seen.insert(i.hash_name())).cloned().collect()
}

// ------------------------------------------------------------ presence

/// Someone else on the open board.
#[derive(Clone, Debug, PartialEq)]
pub struct Peer {
    pub conn: String,
    pub user: User,
    pub role: Role,
    /// Board coordinates of their pointer, and the page they are on.
    pub cursor: Option<(f64, f64)>,
    pub page: String,
    pub selection: Vec<String>,
    pub tool: String,
    /// When their cursor last moved, in app time.
    pub moved_at: u64,
}

impl Peer {
    pub fn from_json(v: &Value, now: u64) -> Option<Peer> {
        let mut p = Peer {
            conn: v["conn"].as_str()?.to_string(),
            user: User::from_json(&v["user"])?,
            role: Role::parse(v["role"].as_str().unwrap_or_default()),
            cursor: None,
            page: String::new(),
            selection: Vec::new(),
            tool: String::new(),
            moved_at: now,
        };
        p.update(v, now);
        Some(p)
    }

    /// Applies a presence message. The page travels as the tool's prefix
    /// ("page|tool") so the room needs no knowledge of pages.
    pub fn update(&mut self, v: &Value, now: u64) {
        let cursor = v["cursor"].as_array().and_then(|c| Some((c.first()?.as_f64()?, c.get(1)?.as_f64()?)));
        if cursor != self.cursor {
            self.moved_at = now;
        }
        self.cursor = cursor;
        let tool = v["tool"].as_str().unwrap_or_default();
        let (page, tool) = tool.split_once('|').unwrap_or(("", tool));
        self.page = page.to_string();
        self.tool = tool.to_string();
        self.selection = v["sel"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect();
    }
}

pub fn presence_message(page: &str, cursor: Option<(f64, f64)>, selection: &[String], tool: &str, view: [f64; 4]) -> Value {
    json!({
        "t": "presence",
        "cursor": cursor.map(|(x, y)| json!([x, y])),
        "sel": selection,
        "tool": format!("{page}|{tool}"),
        "view": view,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Pt;
    use crate::shape::Tool;

    #[test]
    fn records_reuse_unchanged_shapes_and_list_pages() {
        let a = Arc::new(Shape::new("a", Tool::Rectangle, vec![Pt::new(0.0, 0.0), Pt::new(5.0, 5.0)]));
        let b = Arc::new(Shape::new("b", Tool::Ellipse, vec![Pt::new(0.0, 0.0), Pt::new(5.0, 5.0)]));
        let mut cache = RecordCache::default();
        let (r1, _) = cache.records([("p1", &[a.clone()][..]), ("p2", &[b.clone()][..])].into_iter());
        assert_eq!(r1.len(), 3);
        assert_eq!(r1["p1/a"]["shape"]["tool"], "rectangle");
        match parse_record(&r1[PAGES_ID]) {
            Some(Record::Pages(p)) => assert_eq!(p, vec!["p1", "p2"]),
            _ => panic!("pages record"),
        }
        match parse_record(&r1["p2/b"]) {
            Some(Record::Shape { page, shape }) => assert_eq!((page.as_str(), shape["id"].as_str()), ("p2", Some("b"))),
            _ => panic!("shape record"),
        }
        let (r2, _) = cache.records([("p1", &[a][..])].into_iter());
        assert_eq!(r2.len(), 2);
    }

    #[test]
    fn peers_follow_presence() {
        let v = json!({ "conn": "c1", "user": { "id": "u", "name": "Ana Silva", "email": "ana@x.com", "color": "#1971c2" }, "role": "editor",
            "cursor": [1.5, 2.0], "tool": "pg1|pen", "sel": ["s1"] });
        let mut p = Peer::from_json(&v, 10).unwrap();
        assert_eq!((p.cursor, p.page.as_str(), p.tool.as_str()), (Some((1.5, 2.0)), "pg1", "pen"));
        assert_eq!(p.user.color, 0xff1971c2);
        assert_eq!(p.user.initials(), "AS");
        p.update(&json!({ "cursor": null, "tool": "pg2|select", "sel": [] }), 20);
        assert_eq!((p.cursor, p.page.as_str(), p.moved_at), (None, "pg2", 20));
        let m = presence_message("pg1", Some((1.0, 2.0)), &["a".into()], "pen", [0.0, 0.0, 10.0, 10.0]);
        assert_eq!(m["tool"], "pg1|pen");
    }

    #[test]
    fn initials_and_roles() {
        assert_eq!(initials("ben@example.com"), "BE");
        assert_eq!(initials(""), "?");
        assert!(Role::parse("editor").can_edit() && !Role::parse("viewer").can_edit());
    }
}

#[cfg(test)]
mod live_test;
