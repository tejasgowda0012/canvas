//! Team boards: Google Workspace sign-in, the team board list, live editing
//! with everyone on a board, their cursors and selections, who made what,
//! and sharing.
//!
//! A team board is cached on this device as an ordinary board file named
//! `cloud-<server id>`, so it opens instantly and keeps working offline;
//! the `cloud` entry of the file remembers how far it is in step with the
//! server.

use super::{
    random_token, Act, App, AskFor, ButtonKind, ConfirmFor, Face, Ico, Job, Look, MenuKind, Overlay, Page, PageChange, PanelKind,
    PanelRows, Rf, Ui, ACCENT, ACCENT_TINT, BORDER, DANGER, OK, SURFACE, SURFACE2, TEXT1, TEXT2, TEXT3,
};
use crate::geom::Pt;
use crate::import::ImageAsset;
use crate::shape::Shape;
use crate::ui::cloud::replica::{Attribution, Changes, Replica};
use crate::ui::cloud::room::{self, RoomEvent};
use crate::ui::cloud::{self, parse_record, Account, CloudBoard, Peer, Record, Reply, Role, SignInStep};
use crate::ui::font;
use crate::ui::paint::Painter;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Prefix of the local file that caches a team board.
pub(super) const CACHE_PREFIX: &str = "cloud-";
/// Cursors that have not moved for this long fade away.
const CURSOR_IDLE_MS: u64 = 20_000;
/// How long someone else's change stays highlighted.
const FLASH_MS: u64 = 1_800;
/// Smallest gap between presence messages.
const PRESENCE_MS: u64 = 50;

/// Which boards the library lists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Tab {
    All,
    /// Boards the signed-in person owns.
    Yours,
    /// Boards other people made and shared with them.
    Shared,
}

impl Tab {
    pub(super) const ALL: [Tab; 3] = [Tab::All, Tab::Yours, Tab::Shared];

    pub(super) fn label(self) -> &'static str {
        match self {
            Tab::All => "All",
            Tab::Yours => "Yours",
            Tab::Shared => "Shared with you",
        }
    }

    fn keeps(self, board: &CloudBoard) -> bool {
        match self {
            Tab::All => true,
            Tab::Yours => board.role == Role::Owner,
            Tab::Shared => board.role != Role::Owner,
        }
    }
}

/// The name of everyone who can be given access to all of a person's boards.
const ORG: &str = "Kenpath";

/// Who can open a board, as the library's badge says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Access {
    Private,
    Shared,
    Org,
}

impl Access {
    pub(super) fn label(self) -> String {
        match self {
            Access::Private => "Private".into(),
            Access::Shared => "Shared".into(),
            Access::Org => format!("Everyone at {ORG}"),
        }
    }

    pub(super) fn icon(self) -> Ico {
        match self {
            Access::Private => Ico::Lock,
            Access::Shared => Ico::People,
            Access::Org => Ico::Globe,
        }
    }
}

pub(super) struct SignIn {
    cancel: Arc<AtomicBool>,
    pub code: Option<(String, String)>,
    pub error: Option<String>,
    /// Waiting for the browser instead of showing a code.
    pub browser: bool,
}

impl Drop for SignIn {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// A shape someone else just changed.
struct Flash {
    page: String,
    shape: String,
    user: String,
    until: u64,
}

/// The open team board's connection and everyone on it.
pub(super) struct Live {
    pub cloud_id: String,
    room: room::Room,
    replica: Replica,
    cache: cloud::RecordCache,
    images: Vec<Arc<ImageAsset>>,
    pub online: Option<bool>,
    synced: bool,
    pub role: Role,
    pub peers: HashMap<String, Peer>,
    /// Names and colours of everyone who has been on the board.
    people: HashMap<String, (String, u32)>,
    sent_presence: Option<Value>,
    presence_at: u64,
    /// Something others see may have changed since the last presence message.
    presence_due: bool,
    cursor: Option<(f64, f64)>,
    flashes: Vec<Flash>,
    pub refused: Option<String>,
    /// When the last "view only" notice showed.
    view_only_notice: u64,
}

pub(super) fn cache_id(cloud_id: &str) -> String {
    format!("{CACHE_PREFIX}{cloud_id}")
}

/// "3 min ago" style age of a Unix-milliseconds time.
pub(super) fn ago(then_ms: i64) -> String {
    let secs = ((super::wall_millis() - then_ms) / 1000).max(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

fn person_name(name: &str, email: &str) -> String {
    if name.is_empty() { email.to_string() } else { name.to_string() }
}

/// A steady colour for someone whose profile colour is not known here.
pub(super) fn person_color(email: &str) -> u32 {
    const COLORS: [u32; 8] = [0xff2748e8, 0xffe8590c, 0xff1e8e5a, 0xff9c36b5, 0xffd6336c, 0xff0b7285, 0xff7048e8, 0xffc92a2a];
    let hash = email.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    COLORS[hash as usize % COLORS.len()]
}

fn device_name() -> String {
    if cfg!(target_arch = "wasm32") { "Inkboard on the web".into() } else { format!("Inkboard on {}", std::env::consts::OS) }
}

impl App {
    pub(super) fn account(&self) -> Option<&Account> {
        self.settings.account.as_ref()
    }

    pub(super) fn me(&self) -> Option<&cloud::User> {
        self.account().map(|a| &a.user)
    }

    // ------------------------------------------------------------ account

    pub(super) fn start_sign_in(&mut self) {
        let cancel = Arc::new(AtomicBool::new(false));
        let server = self.settings.server.clone();
        // A browser does it in one step; panels without one show a code to enter elsewhere.
        let browser = self.platform.sign_in_in_browser(&server);
        self.sign_in = Some(SignIn { cancel: cancel.clone(), code: None, error: None, browser });
        if !browser {
            cloud::sign_in(server, device_name(), cancel, self.cloud_tx.clone(), self.wake.clone());
        }
        self.panel(PanelKind::Account);
    }

    /// Starts signing in with a code on another device, when the browser
    /// isn't an option (or didn't work).
    pub(super) fn sign_in_with_code(&mut self) {
        let cancel = Arc::new(AtomicBool::new(false));
        self.sign_in = Some(SignIn { cancel: cancel.clone(), code: None, error: None, browser: false });
        cloud::sign_in(self.settings.server.clone(), device_name(), cancel, self.cloud_tx.clone(), self.wake.clone());
        self.panel(PanelKind::Account);
    }

    /// The host finished "Sign in with Google" in a browser with this
    /// session token; the app looks up who it belongs to.
    pub fn finish_sign_in(&mut self, token: &str) {
        let (server, token) = (self.settings.server.clone(), token.to_string());
        cloud::spawn(&self.cloud_tx, &self.wake, async move {
            match cloud::http::api(&server, "GET", "/api/me", Some(&token), None).await.map(|v| cloud::User::from_json(&v)) {
                Ok(Some(user)) => Reply::SignIn(SignInStep::Done(Account { server, token, user })),
                Ok(None) => Reply::SignIn(SignInStep::Failed("Sign-in failed. Try again.".into())),
                Err(e) => Reply::SignIn(SignInStep::Failed(e.message)),
            }
        });
    }

    /// The browser sign-in failed or was cancelled.
    pub fn sign_in_failed(&mut self, message: &str) {
        match &mut self.sign_in {
            Some(s) => s.error = Some(message.to_string()),
            None => self.error_toast(message),
        }
    }

    /// Opens a board from a link (`/app/#board=<id>`), signing in first if needed.
    pub fn open_board_link(&mut self, cloud_id: &str) {
        let valid = cloud_id.len() == 36 && cloud_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
        if !valid {
            return;
        }
        if self.account().is_none() {
            self.pending_link = Some(cloud_id.to_string());
            self.start_sign_in();
            return;
        }
        self.open_team(cloud_id);
    }

    /// Moves the next board that only lives on this device into the
    /// person's account, one at a time, once signed in.
    pub(super) fn move_next_local_board(&mut self) {
        if self.account().is_none() || !self.listed || self.moving.is_some() || self.copying.is_some() {
            return;
        }
        let open = self.board.as_ref().map(|b| b.id.clone());
        let Some(meta) = self.boards.iter().find(|m| {
            !m.id.starts_with(CACHE_PREFIX) && Some(&m.id) != open.as_ref() && !self.move_failed.contains(&m.id)
        }) else {
            return;
        };
        let (from, title) = (meta.id.clone(), meta.title.clone());
        self.moving = Some((from, String::new()));
        self.request(move |a| async move { Reply::Created(cloud::create_board(&a, &title).await) });
    }

    fn sign_out(&mut self) {
        if let Some(a) = self.settings.account.clone() {
            cloud::run(async move { cloud::sign_out(&a).await });
        }
        self.forget_account();
        self.dismiss();
        self.show_toast("Signed out");
    }

    fn forget_account(&mut self) {
        self.set(|s| s.account = None);
        self.team_boards.clear();
        self.team_listed = false;
        self.live = None;
    }

    pub(super) fn refresh_team(&mut self) {
        let Some(a) = self.account().cloned() else { return };
        cloud::spawn(&self.cloud_tx, &self.wake, async move { Reply::Boards(cloud::list_boards(&a).await) });
    }

    fn request<F: std::future::Future<Output = Reply> + cloud::MaybeSend + 'static>(&mut self, job: impl FnOnce(Account) -> F) {
        let Some(a) = self.account().cloned() else {
            self.start_sign_in();
            return;
        };
        cloud::spawn(&self.cloud_tx, &self.wake, job(a));
    }

    fn api_error(&mut self, e: &cloud::http::ApiError) {
        if e.signed_out() {
            self.forget_account();
            self.set_toast("Your sign-in expired.", true, Some(("Sign in", Act::SignIn)));
        } else {
            self.error_toast(&e.message);
        }
    }

    /// Handles finished server requests.
    pub(super) fn take_cloud(&mut self) {
        while let Ok(reply) = self.cloud_rx.try_recv() {
            match reply {
                Reply::SignIn(step) => match step {
                    SignInStep::Code { code, url } => {
                        if let Some(s) = &mut self.sign_in {
                            s.code = Some((code, url));
                        }
                    }
                    SignInStep::Done(account) => {
                        self.sign_in = None;
                        let name = account.user.first_name().to_string();
                        self.set(|s| s.account = Some(account));
                        self.tab = Tab::All;
                        self.refresh_team();
                        if matches!(self.overlay, Some(Overlay::Panel { kind: PanelKind::Account, .. })) {
                            self.dismiss();
                        }
                        self.show_toast(&format!("Signed in. Welcome, {name}"));
                        if self.live.as_ref().is_some_and(|l| l.refused.is_some()) {
                            self.start_live();
                        }
                        if let Some(id) = self.pending_link.take() {
                            self.open_team(&id);
                        }
                        self.move_next_local_board();
                    }
                    SignInStep::Failed(message) => {
                        if let Some(s) = &mut self.sign_in {
                            s.error = Some(message);
                            s.code = None;
                        }
                    }
                },
                Reply::Boards(Ok(list)) => {
                    self.team_boards = list;
                    self.team_listed = true;
                    self.clamp_scroll();
                }
                Reply::Boards(Err(e)) => {
                    self.team_listed = true;
                    if e.signed_out() {
                        self.api_error(&e);
                    }
                }
                Reply::Created(Ok(board)) if self.moving.as_ref().is_some_and(|(_, to)| to.is_empty()) => {
                    self.team_boards.insert(0, board.clone());
                    let from = self.moving.as_ref().unwrap().0.clone();
                    let to = cache_id(&board.id);
                    self.moving = Some((from.clone(), to.clone()));
                    self.send(Job::Copy { from, to, title: board.title.clone(), cloud: board.id.clone() });
                }
                Reply::Created(Err(e)) if self.moving.as_ref().is_some_and(|(_, to)| to.is_empty()) => {
                    let (from, _) = self.moving.take().unwrap();
                    self.move_failed.push(from);
                    if e.signed_out() {
                        self.api_error(&e);
                    }
                }
                Reply::Created(Ok(board)) => {
                    self.team_boards.insert(0, board.clone());
                    if let Some(copy) = self.copying.take() {
                        self.send(Job::Copy { from: copy, to: cache_id(&board.id), title: board.title.clone(), cloud: board.id.clone() });
                    } else {
                        self.open_team(&board.id);
                    }
                }
                Reply::Created(Err(e)) => {
                    self.copying = None;
                    self.api_error(&e);
                }
                Reply::Members(id, Ok(m)) => {
                    if let Some(b) = self.team_boards.iter_mut().find(|b| b.id == id) {
                        b.org_access = m.org_access.clone();
                    }
                    if m.members.is_empty() {
                        self.invited.remove(&id);
                    } else {
                        self.invited.insert(id.clone());
                    }
                    self.members = Some((id, m));
                }
                Reply::Members(_, Err(e)) => self.api_error(&e),
                Reply::Board(id, Ok(board)) => {
                    if let Some(open) = self.board.as_mut().filter(|b| b.id == cache_id(&id)) {
                        open.title = board.title.clone();
                        self.unsaved = true;
                        self.save_due = Some(self.now + 300);
                    }
                    if let Some((mid, m)) = &mut self.members {
                        if *mid == id {
                            m.org_access = board.org_access.clone();
                        }
                    }
                    match self.team_boards.iter_mut().find(|b| b.id == id) {
                        Some(b) => *b = board,
                        None => self.team_boards.insert(0, board),
                    }
                }
                Reply::Board(_, Err(e)) => self.api_error(&e),
                Reply::Deleted(id, Ok(())) => {
                    self.team_boards.retain(|b| b.id != id);
                    let local = cache_id(&id);
                    self.thumbs.remove(&local);
                    self.send(Job::Delete(local));
                    self.show_toast("Board deleted");
                }
                Reply::Deleted(_, Err(e)) => self.api_error(&e),
            }
        }
    }

    // ------------------------------------------------------------ opening

    pub(super) fn open_team(&mut self, cloud_id: &str) {
        let local = cache_id(cloud_id);
        if self.boards.iter().any(|b| b.id == local) {
            self.open(local);
            return;
        }
        let title = self.team_boards.iter().find(|b| b.id == cloud_id).map_or("Team board".to_string(), |b| b.title.clone());
        let mut extra = Map::new();
        extra.insert("cloud".into(), json!({ "id": cloud_id }));
        let updated = self.timestamp();
        self.send(Job::Create { id: local, title, updated, extra });
    }

    /// Connects the open board to its room, if it is a team board.
    pub(super) fn start_live(&mut self) {
        self.live = None;
        let Some(board) = &self.board else { return };
        let Some(state) = board.extra.get("cloud").cloned() else { return };
        let Some(cloud_id) = state["id"].as_str().map(str::to_string) else { return };
        let Some(account) = self.account().cloned() else {
            self.set_toast("Sign in to keep this team board in step.", true, Some(("Sign in", Act::SignIn)));
            return;
        };
        let attribution: HashMap<String, Attribution> = state["attrib"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(id, a)| {
                Some((id.clone(), Attribution {
                    created_by: a.get(0)?.as_str()?.to_string(),
                    edited_by: a.get(1)?.as_str()?.to_string(),
                    edited_at: a.get(2)?.as_i64()?,
                }))
            })
            .collect();
        let people = state["people"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(id, p)| Some((id.clone(), (p.get(0)?.as_str()?.to_string(), p.get(1)?.as_u64()? as u32))))
            .collect();
        let mut cache = cloud::RecordCache::default();
        let (mut known, _) = cache.records(self.page_shapes());
        // Changes made here but never confirmed are sent again.
        for id in state["pending"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if known.remove(id).is_none() {
                known.insert(id.to_string(), json!({ "id": id }));
            }
        }
        if state["rev"].as_u64().unwrap_or(0) == 0 {
            // Never synced: the room's copy decides, plus anything drawn here.
            known.clear();
        }
        let replica = Replica::new(
            state["epoch"].as_str().unwrap_or_default().to_string(),
            state["rev"].as_u64().unwrap_or(0),
            known,
            attribution,
        );
        let room = room::open(account.server.clone(), account.token.clone(), cloud_id.clone(), self.images.clone(), self.wake.clone());
        let role = Role::parse(state["role"].as_str().unwrap_or("editor"));
        self.live = Some(Live {
            cloud_id,
            room,
            replica,
            cache,
            images: Vec::new(),
            online: None,
            synced: false,
            role,
            peers: HashMap::new(),
            people,
            sent_presence: None,
            presence_at: 0,
            presence_due: true,
            cursor: None,
            flashes: Vec::new(),
            refused: None,
            view_only_notice: 0,
        });
        // "Share…" on a card opens the board with the share panel on top.
        if self.share_on_open.as_deref() == self.live.as_ref().map(|l| l.cloud_id.as_str()) {
            self.share_on_open = None;
            self.share_panel();
        }
    }

    /// What the board file keeps about its server copy.
    pub(super) fn cloud_state(&self) -> Option<Value> {
        let live = self.live.as_ref()?;
        let r = &live.replica;
        let attrib: Map<String, Value> = r
            .attribution
            .iter()
            .map(|(id, a)| (id.clone(), json!([a.created_by, a.edited_by, a.edited_at])))
            .collect();
        let people: Map<String, Value> = live.people.iter().map(|(id, (n, c))| (id.clone(), json!([n, c]))).collect();
        Some(json!({
            "id": live.cloud_id,
            "epoch": r.epoch,
            "rev": if live.synced || r.rev > 0 { r.rev } else { 0 },
            "role": live.role.name(),
            "attrib": attrib,
            "people": people,
            "pending": r.pending_ids(),
        }))
    }

    /// Every page's key and shapes, the open page from the editor.
    fn page_shapes(&self) -> impl Iterator<Item = (&str, &[Arc<Shape>])> {
        self.pages.iter().enumerate().map(move |(i, p)| {
            let shapes: &[Arc<Shape>] = if i == self.page { self.ed.doc.shapes() } else { &p.shapes };
            (p.key.as_str(), shapes)
        })
    }

    fn records(&mut self) -> Option<HashMap<String, Value>> {
        let mut cache = std::mem::take(&mut self.live.as_mut()?.cache);
        let (records, images) = cache.records(self.page_shapes());
        let live = self.live.as_mut()?;
        live.cache = cache;
        live.images = images;
        Some(records)
    }

    // --------------------------------------------------------------- sync

    /// Sends this device's changes to the room.
    pub(super) fn push_local(&mut self) {
        if !self.live.as_ref().is_some_and(|l| l.synced && l.online == Some(true)) {
            return;
        }
        let Some(records) = self.records() else { return };
        let live = self.live.as_mut().unwrap();
        if !live.role.can_edit() {
            // Viewers' changes are put back.
            let known = live.replica.known().clone();
            let changes = Changes {
                put: known.iter().filter(|(id, v)| records.get(*id) != Some(*v)).map(|(_, v)| v.clone()).collect(),
                del: records.keys().filter(|id| !known.contains_key(*id)).cloned().collect(),
            };
            if !changes.is_empty() {
                if self.now >= live.view_only_notice + 4000 {
                    live.view_only_notice = self.now;
                    self.show_toast("You can view this board but not change it");
                }
                self.apply_changes(changes, None);
            }
            return;
        }
        let batches = live.replica.local(&records);
        for batch in batches {
            let images = cloud::images_for(&batch.put, &live.images);
            live.room.send_with_images(batch.to_message(), images);
        }
    }

    /// Handles everything the room sent since the last frame.
    pub(super) fn take_room(&mut self) {
        loop {
            let Some(event) = self.live.as_ref().and_then(|l| l.room.events.try_recv().ok()) else { return };
            self.room_event(event);
        }
    }

    fn room_event(&mut self, event: RoomEvent) {
        let me = self.me().map(|u| u.id.clone()).unwrap_or_default();
        let Some(live) = self.live.as_mut() else { return };
        match event {
            RoomEvent::Online => {
                live.online = Some(true);
                live.refused = None;
                live.room.send(json!({ "t": "hello", "rev": live.replica.rev, "epoch": live.replica.epoch }));
                self.full = true;
            }
            RoomEvent::Offline(_) => {
                live.online = Some(false);
                live.synced = false;
                live.peers.clear();
                live.sent_presence = None;
                self.full = true;
            }
            RoomEvent::Refused { status, message } => {
                live.online = Some(false);
                live.synced = false;
                live.peers.clear();
                live.refused = Some(message.clone());
                self.full = true;
                if status == 401 {
                    self.forget_account();
                    self.set_toast("Your sign-in expired.", true, Some(("Sign in", Act::SignIn)));
                } else {
                    self.error_toast(&message);
                }
            }
            RoomEvent::Message(m) => self.room_message(m, &me),
        }
    }

    fn room_message(&mut self, m: Value, me: &str) {
        let now = self.now;
        let Some(live) = self.live.as_mut() else { return };
        match m["t"].as_str().unwrap_or_default() {
            "welcome" => {
                live.role = Role::parse(m["you"]["role"].as_str().unwrap_or_default());
                for (id, p) in m["people"].as_object().into_iter().flatten() {
                    live.people.insert(id.clone(), (p["name"].as_str().unwrap_or_default().to_string(), cloud::parse_color(p["color"].as_str().unwrap_or_default())));
                }
                live.peers = m["peers"].as_array().into_iter().flatten().filter_map(|p| Peer::from_json(p, now)).map(|p| (p.conn.clone(), p)).collect();
                live.replica.welcome(m["epoch"].as_str().unwrap_or_default(), m["full"].as_bool().unwrap_or(true));
                self.full = true;
            }
            "sync" => {
                let changes = live.replica.remote(m["items"].as_array().map_or(&[][..], |v| v), m["del"].as_array().map_or(&[][..], |v| v), None);
                self.apply_changes(changes, None);
            }
            "synced" => {
                let changes = live.replica.synced(m["rev"].as_u64().unwrap_or(0));
                live.synced = true;
                self.apply_changes(changes, None);
                // Anything changed here while away goes up now.
                if let Some(records) = self.records() {
                    let live = self.live.as_mut().unwrap();
                    if live.role.can_edit() {
                        let batches = live.replica.resend(&records);
                        for batch in batches {
                            let images = cloud::images_for(&batch.put, &live.images);
                            live.room.send_with_images(batch.to_message(), images);
                        }
                    }
                }
                self.push_local();
                self.mark_unsaved();
            }
            "ops" => {
                let by = m["by"].as_str().unwrap_or_default().to_string();
                let items = m["items"].as_array().cloned().unwrap_or_default();
                let changes = live.replica.remote(&items, m["del"].as_array().map_or(&[][..], |v| v), m["rev"].as_u64());
                if by != me {
                    for item in &items {
                        if let Some(Record::Shape { page, shape }) = parse_record(&item["s"]) {
                            live.flashes.push(Flash { page, shape: shape["id"].as_str().unwrap_or_default().to_string(), user: by.clone(), until: now + FLASH_MS });
                        }
                    }
                }
                self.apply_changes(changes, Some(by));
                self.mark_unsaved();
            }
            "ack" => {
                live.replica.ack(m["id"].as_u64().unwrap_or(0), m["rev"].as_u64().unwrap_or(0), m["at"].as_i64().unwrap_or(0), me);
                self.mark_unsaved();
            }
            "presence" => {
                if let Some(p) = live.peers.get_mut(m["conn"].as_str().unwrap_or_default()) {
                    p.update(&m, now);
                }
            }
            "join" => {
                if let Some(p) = Peer::from_json(&m, now) {
                    live.people.insert(p.user.id.clone(), (p.user.name.clone(), p.user.color));
                    live.peers.insert(p.conn.clone(), p);
                    self.full = true;
                }
            }
            "leave" => {
                live.peers.remove(m["conn"].as_str().unwrap_or_default());
                self.full = true;
            }
            "error" => {
                let message = m["message"].as_str().unwrap_or("The board refused a change").to_string();
                if m["code"] == "read_only" {
                    live.role = Role::Viewer;
                }
                self.error_toast(&message);
            }
            _ => {}
        }
    }

    /// Applies changes from the room to the pages and the editor.
    fn apply_changes(&mut self, changes: Changes, _by: Option<String>) {
        if changes.is_empty() {
            return;
        }
        let images = self.images.clone();
        let resolve = |name: &str| images.lock().ok().and_then(|m| m.get(name).cloned());
        let current_key = self.pages.get(self.page).map(|p| p.key.clone()).unwrap_or_default();
        let mut order: Option<Vec<String>> = None;
        let mut put: HashMap<String, Vec<Shape>> = HashMap::new();
        let mut del: HashMap<String, Vec<String>> = HashMap::new();
        let mut broken = Vec::new();
        for record in &changes.put {
            match parse_record(record) {
                Some(Record::Pages(keys)) => order = Some(keys),
                Some(Record::Shape { page, shape }) => match Shape::from_json_with_images(&shape, &resolve) {
                    Ok(s) => put.entry(page).or_default().push(s),
                    Err(_) => broken.push(record["id"].as_str().unwrap_or_default().to_string()),
                },
                None => {}
            }
        }
        for id in &changes.del {
            if let Some((page, shape)) = id.split_once('/') {
                del.entry(page.to_string()).or_default().push(shape.to_string());
            }
        }
        if let Some(live) = self.live.as_mut() {
            // Shapes this device cannot show are left alone rather than deleted.
            for id in broken {
                live.replica.forget(&id);
            }
        }

        let mut reload = false;
        if order.is_some() || put.keys().chain(del.keys()).any(|k| !self.pages.iter().any(|p| &p.key == k)) {
            self.store_page();
            let mut pages: HashMap<String, Page> = self.pages.drain(..).map(|p| (p.key.clone(), p)).collect();
            let mut keys = order.unwrap_or_else(|| {
                let mut k: Vec<String> = pages.keys().cloned().collect();
                k.sort();
                k
            });
            for k in put.keys() {
                if !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
            let mut next: Vec<Page> = keys
                .iter()
                .map(|k| pages.remove(k).unwrap_or_else(|| Page { key: k.clone(), ..Page::default() }))
                .collect();
            if next.is_empty() {
                next.push(Page { key: current_key.clone(), ..Page::default() });
            }
            self.pages = next;
            match self.pages.iter().position(|p| p.key == current_key) {
                Some(i) => self.page = i,
                None => {
                    self.page = self.page.min(self.pages.len() - 1);
                    reload = true;
                }
            }
        }
        let open_key = self.pages[self.page].key.clone();
        for (i, page) in self.pages.iter_mut().enumerate() {
            let adds = put.remove(&page.key).unwrap_or_default();
            let removes = del.remove(&page.key).unwrap_or_default();
            if adds.is_empty() && removes.is_empty() {
                continue;
            }
            if i == self.page && !reload {
                self.ed.apply_remote(adds, removes);
                continue;
            }
            page.shapes.retain(|s| !removes.contains(&s.id));
            for s in adds {
                match page.shapes.iter().position(|old| old.id == s.id) {
                    Some(at) => page.shapes[at] = Arc::new(s),
                    None => page.shapes.push(Arc::new(s)),
                }
            }
        }
        if reload {
            let shapes = self.pages[self.page].shapes.iter().map(|s| (**s).clone()).collect();
            let _ = self.ed.load(shapes);
            self.restore_view();
            self.doc_gen += 1;
            self.full = true;
            let _ = open_key;
        }
        self.take_editor_events();
        self.mark_unsaved();
    }

    // ----------------------------------------------------------- presence

    /// Where this person's pointer is, in screen pixels.
    pub(super) fn track_pointer(&mut self, x: f32, y: f32) {
        let view = self.ed.screen_view();
        let Some(live) = self.live.as_mut() else { return };
        let world = self.ed.world(Pt::new(x as f64 / view.dpr, y as f64 / view.dpr));
        live.cursor = world.is_finite().then_some((world.x, world.y));
        live.presence_due = true;
    }

    pub(super) fn forget_pointer(&mut self) {
        if let Some(live) = self.live.as_mut() {
            live.cursor = None;
            live.presence_due = true;
        }
    }

    /// Tells the others where this person is, at most every [`PRESENCE_MS`].
    pub(super) fn send_presence(&mut self) {
        let now = self.now;
        let page = self.pages.get(self.page).map(|p| p.key.clone()).unwrap_or_default();
        let selection = self.ed.doc.selected_ids().to_vec();
        let tool = format!("{:?}", self.ed.tool()).to_lowercase();
        let view = self.ed.screen_view();
        let (w, h) = (self.w as f64 / view.dpr, self.h as f64 / view.dpr);
        let tl = self.ed.world(Pt::ZERO);
        let br = self.ed.world(Pt::new(w, h));
        let Some(live) = self.live.as_mut() else { return };
        live.flashes.retain(|f| f.until > now);
        if !live.synced || live.online != Some(true) {
            return;
        }
        if now < live.presence_at + PRESENCE_MS {
            live.presence_due = true;
            return;
        }
        live.presence_due = false;
        let msg = cloud::presence_message(&page, live.cursor, &selection, &tool, [tl.x, tl.y, br.x, br.y]);
        if live.sent_presence.as_ref() != Some(&msg) {
            live.room.send(msg.clone());
            live.sent_presence = Some(msg);
            live.presence_at = now;
        }
    }

    /// When something about the others changes on its own (cursor fading,
    /// highlights ending).
    pub(super) fn live_deadline(&self) -> Option<u64> {
        let live = self.live.as_ref()?;
        let flash = live.flashes.iter().map(|f| f.until).min();
        let fade = live.peers.values().filter(|p| p.cursor.is_some()).map(|p| p.moved_at + CURSOR_IDLE_MS).filter(|t| *t > self.now).min();
        let presence = (live.presence_due && live.synced).then_some((live.presence_at + PRESENCE_MS).max(self.now));
        [flash, fade, presence].into_iter().flatten().min()
    }

    // ------------------------------------------------------------- layout

    fn to_ui(&self, x: f64, y: f64, u: f32) -> (f32, f32) {
        let v = self.ed.screen_view();
        let sx = (x * v.zoom + v.cam.x) * v.dpr;
        let sy = (y * v.zoom + v.cam.y) * v.dpr;
        (sx as f32 / u, sy as f32 / u)
    }

    fn name_of(&self, user: &str) -> (String, u32) {
        if self.me().is_some_and(|m| m.id == user) {
            return ("You".into(), ACCENT);
        }
        if let Some(live) = &self.live {
            if let Some(p) = live.peers.values().find(|p| p.user.id == user) {
                return (p.user.first_name().to_string(), p.user.color);
            }
            if let Some((name, color)) = live.people.get(user) {
                let first = name.split_whitespace().next().unwrap_or(name).to_string();
                return (first, *color);
            }
        }
        ("Someone".into(), TEXT3)
    }

    fn outline_for(&self, ui: &mut Ui, ids: &[String], color: u32, label: String) {
        let shapes: Vec<_> = self.ed.doc.shapes().iter().filter(|s| ids.contains(&s.id)).cloned().collect();
        let Some(b) = crate::document::scene_bounds(&shapes) else { return };
        let (x0, y0) = self.to_ui(b.l, b.t, ui.u);
        let (x1, y1) = self.to_ui(b.r, b.b, ui.u);
        let pad = 6.0;
        let (w, h) = ((x1 - x0 + 2.0 * pad).max(40.0), (y1 - y0 + 2.0 * pad).max(12.0) + 30.0);
        // The name sits above the box, or below it at the top of the screen.
        let below = y0 - pad - 30.0 < 4.0;
        let y = if below { y0 - pad } else { y0 - pad - 30.0 };
        ui.push(Rf::new(x0 - pad, y, w, h), Look::Outline { color, label, below }, None);
    }

    /// Others' selections, recent changes and cursors, under the toolbars.
    pub(super) fn layout_live(&self, ui: &mut Ui) {
        let Some(live) = &self.live else { return };
        let page = self.pages.get(self.page).map(|p| p.key.as_str()).unwrap_or_default();
        for p in live.peers.values().filter(|p| p.page == page && !p.selection.is_empty()) {
            self.outline_for(ui, &p.selection, p.user.color, p.user.first_name().to_string());
        }
        for f in live.flashes.iter().filter(|f| f.page == page) {
            let (name, color) = self.name_of(&f.user);
            self.outline_for(ui, std::slice::from_ref(&f.shape), color, name);
        }
        let (w, h) = self.extent();
        for p in live.peers.values().filter(|p| p.page == page) {
            let Some((x, y)) = p.cursor else { continue };
            if self.now >= p.moved_at + CURSOR_IDLE_MS {
                continue;
            }
            let (x, y) = self.to_ui(x, y, ui.u);
            if x < -40.0 || y < -40.0 || x > w || y > h {
                continue;
            }
            let name = p.user.first_name().to_string();
            let width = 40.0 + font::measure(Face::Medium, 20.0, &name).0 + 28.0;
            ui.push(Rf::new(x, y, width, 64.0), Look::Cursor { color: p.user.color, name }, None);
        }
    }

    /// Avatars of everyone on the board, and the connection state.
    pub(super) fn layout_people(&self, ui: &mut Ui, r: Rf) {
        let Some(live) = &self.live else { return };
        let mut users: Vec<&cloud::User> = Vec::new();
        for p in live.peers.values() {
            if !users.iter().any(|u| u.id == p.user.id) && self.me().map_or(true, |m| m.id != p.user.id) {
                users.push(&p.user);
            }
        }
        users.sort_by(|a, b| a.name.cmp(&b.name));
        let size = (r.h - 16.0).min(56.0);
        let shown = users.len().min(((r.w - 120.0) / (size * 0.7)).max(0.0) as usize).min(4);
        let mut x = r.x + 8.0;
        let y = r.y + (r.h - size) / 2.0;
        if let Some(me) = self.me() {
            ui.push(Rf::new(x, y, size, size), Look::Avatar { text: me.initials(), color: me.color }, Some(Act::Share));
            x += size * 0.72;
        }
        for u in users.iter().take(shown) {
            ui.push(Rf::new(x, y, size, size), Look::Avatar { text: u.initials(), color: u.color }, Some(Act::Share));
            x += size * 0.72;
        }
        if users.len() > shown {
            ui.push(Rf::new(x, y, size, size), Look::Avatar { text: format!("+{}", users.len() - shown), color: TEXT3 }, Some(Act::Share));
            x += size * 0.72;
        }
        let (text, online) = match (&live.refused, live.online) {
            (Some(_), _) => ("No access".to_string(), false),
            (None, Some(true)) if live.synced => (if users.is_empty() { "Live".into() } else { format!("{} here", users.len() + 1) }, true),
            (None, Some(false)) => ("Offline".into(), false),
            _ => ("Connecting…".into(), true),
        };
        let left = x + size * 0.28 + 8.0;
        if r.right() - left > 60.0 {
            ui.push(
                Rf::new(left, r.y + (r.h - 48.0) / 2.0, r.right() - left - 4.0, 48.0),
                Look::StatusDot { text, online, chip: true },
                Some(Act::Share),
            );
        }
    }

    /// "Added by Ana · edited by Ben 2 min ago" for a single selected shape.
    pub(super) fn attribution_line(&self) -> Option<String> {
        let live = self.live.as_ref()?;
        let [id] = self.ed.doc.selected_ids() else { return None };
        let page = &self.pages.get(self.page)?.key;
        let a = live.replica.attribution.get(&cloud::record_id(page, id))?;
        let (made, _) = self.name_of(&a.created_by);
        if a.edited_by.is_empty() || a.edited_by == a.created_by {
            return Some(format!("Added by {made} · {}", ago(a.edited_at)));
        }
        let (edited, _) = self.name_of(&a.edited_by);
        Some(format!("Added by {made} · edited by {edited} {}", ago(a.edited_at)))
    }

    // ------------------------------------------------------------ library

    /// Who can open one of the signed-in person's boards.
    pub(super) fn access_of(&self, b: &CloudBoard) -> Access {
        if b.org_access == "view" || b.org_access == "edit" {
            Access::Org
        } else if b.role != Role::Owner || b.members > 0 || self.invited.contains(&b.id) {
            Access::Shared
        } else {
            Access::Private
        }
    }

    // -------------------------------------------------------------- panels

    pub(super) fn account_rows(&self, content: &mut PanelRows) {
        let button = |text: &str, kind: ButtonKind, act: Act| (72.0, Look::Button { icon: None, text: text.into(), kind, pill: false, level: 0 }, Some(act));
        if let Some(a) = self.account() {
            content.push(84.0, Look::Avatar { text: a.user.initials(), color: a.user.color }, None);
            content.y += 8.0;
            content.push(40.0, Look::Label { text: a.user.name.clone(), face: Face::Bold, size: 28.0, color: TEXT1, center: true }, None);
            content.push(36.0, Look::Label { text: a.user.email.clone(), face: Face::Regular, size: 22.0, color: TEXT2, center: true }, None);
            content.y += 24.0;
            content.para("Your boards are saved to your account and open on every device where you sign in.", TEXT2);
            let (h, look, act) = button("Sign out", ButtonKind::DangerTint, Act::SignOut);
            content.push(h, look, act);
            return;
        }
        let Some(s) = &self.sign_in else {
            content.para(&format!("Sign in with your {ORG} Google Workspace account. Every board you make is saved to it, and stays private until you share it."), TEXT2);
            let (h, look, act) = button("Sign in with Google", ButtonKind::Primary, Act::SignIn);
            content.push(h, look, act);
            return;
        };
        if let Some(error) = &s.error {
            content.para(error, DANGER);
            let (h, look, act) = button("Try again", ButtonKind::Primary, Act::SignIn);
            content.push(h, look, act);
            return;
        }
        if s.browser {
            content.para("Continue in your browser: choose your Google account there, then come back here.", TEXT2);
            content.status("Waiting for Google…", true);
            let (h, look, act) = button("Use a code instead", ButtonKind::White, Act::SignInCode);
            content.push(h, look, act);
            return;
        }
        match &s.code {
            None => content.status("Contacting Google…", true),
            Some((code, url)) => {
                content.para(&format!("On your phone or computer, open the address below (or scan the code) and enter this code. Use your {ORG} Google account."), TEXT2);
                content.push(120.0, Look::Label { text: code.clone(), face: Face::Bold, size: 72.0, color: TEXT1, center: true }, Some(Act::Copy(code.clone())));
                content.y += 8.0;
                content.qr(url);
                content.copy_code(url);
                if cfg!(target_arch = "wasm32") {
                    let (h, look, act) = button("Open the sign-in page", ButtonKind::Primary, Act::OpenUrl(url.clone()));
                    content.push(h, look, act);
                }
                content.status("Waiting for you to approve…", true);
            }
        }
    }

    pub(super) fn share_rows(&self, content: &mut PanelRows) {
        let Some(live) = &self.live else {
            if self.me().is_none() {
                content.para("Sign in to share this board. Everyone you invite sees each other's cursors and changes as they happen.", TEXT2);
                content.push(72.0, Look::Button { icon: Some(Ico::Join), text: "Sign in to share".into(), kind: ButtonKind::Primary, pill: false, level: 0 }, Some(Act::SignIn));
            } else {
                content.status("Getting this board ready to share…", true);
            }
            return;
        };
        if let Some(msg) = &live.refused {
            content.para(msg, DANGER);
            return;
        }
        let owner = live.role == Role::Owner;
        let members = self.members.as_ref().filter(|(id, _)| *id == live.cloud_id).map(|(_, m)| m);
        if owner {
            // Like Figma's share dialog: invite first, then who has access.
            content.push(72.0, Look::Button { icon: Some(Ico::Plus), text: "Invite by email".into(), kind: ButtonKind::Primary, pill: false, level: 0 }, Some(Act::AddPerson));
            content.y += 28.0;
        }
        let Some(m) = members else {
            content.status("Loading who has access…", true);
            return;
        };
        content.heading("PEOPLE WITH ACCESS");
        if let Some(o) = &m.owner {
            content.push(84.0, Look::Person { name: person_name(&o.name, &o.email), email: o.email.clone(), role: "Owner".into(), menu: false }, None);
        }
        for member in &m.members {
            let role = if member.role == Role::Editor { "Can edit" } else { "Can view" };
            let act = owner.then(|| Act::MemberMenu(member.email.clone()));
            content.push(84.0, Look::Person { name: person_name(&member.name, &member.email), email: member.email.clone(), role: role.into(), menu: owner }, act);
        }
        if m.members.is_empty() {
            content.para(if owner { "Only you for now. Invite people above, or give everyone at Kenpath access below." } else { "Nobody else has been invited yet." }, TEXT3);
        }
        content.y += 12.0;

        content.heading("GENERAL ACCESS");
        let (access, level) = match m.org_access.as_str() {
            "edit" => (2, "can edit"),
            "view" => (1, "can view"),
            _ => (0, ""),
        };
        let (icon, title) = if access == 0 { (Ico::Lock, "Only people invited".to_string()) } else { (Ico::Globe, format!("Everyone at {ORG}")) };
        content.push(72.0, Look::Row { icon: Some(icon), text: title, value: level.into(), chevron: false, danger: false }, None);
        if owner {
            content.y += 4.0;
            let track = Rf::new(0.0, content.y, content.width, 64.0);
            content.rows.extend(super::segment(
                track,
                [("Only invited", Act::OrgAccess("none")), ("Can view", Act::OrgAccess("view")), ("Can edit", Act::OrgAccess("edit"))],
                access,
            ));
            content.y += 80.0;
        } else {
            content.y += 12.0;
        }

        if !live.peers.is_empty() {
            content.heading("ON THIS BOARD NOW");
            let mut seen = Vec::new();
            for p in live.peers.values() {
                if seen.contains(&p.user.id) {
                    continue;
                }
                seen.push(p.user.id.clone());
                content.push(72.0, Look::Row { icon: None, text: p.user.name.clone(), value: p.role.name().into(), chevron: false, danger: false }, None);
            }
        }
    }

    // ------------------------------------------------------------ actions

    pub(super) fn share_panel(&mut self) {
        if let Some(live) = &self.live {
            let id = live.cloud_id.clone();
            self.request(move |a| async move { Reply::Members(id.clone(), cloud::members(&a, &id).await) });
        }
        self.panel(PanelKind::Share);
    }

    /// Runs team actions; returns false for others.
    pub(super) fn run_team(&mut self, act: &Act) -> bool {
        match act {
            Act::SignIn => self.start_sign_in(),
            Act::Account => {
                if self.account().is_none() && self.sign_in.is_none() {
                    self.start_sign_in();
                } else {
                    self.panel(PanelKind::Account);
                }
            }
            Act::SignOut => self.sign_out(),
            Act::AccountMenu => self.menu(MenuKind::Account, act),
            Act::Filter(tab) => {
                self.tab = *tab;
                self.scroll = 0.0;
            }
            Act::OpenTeam(id) => self.open_team(id),
            Act::TeamMenu(id) => self.menu(MenuKind::TeamCard(id.clone()), act),
            Act::ShareTeam(id) => {
                self.dismiss();
                self.share_on_open = Some(id.clone());
                self.open_team(id);
            }
            Act::CopyBoardLink(id) => {
                let link = format!("{}/app/#board={id}", self.settings.server.trim_end_matches('/'));
                self.platform.copy_text(&link);
                self.show_toast("Link copied");
            }
            Act::RenameTeam(id) => {
                let title = self.team_boards.iter().find(|b| &b.id == id).map(|b| b.title.clone()).unwrap_or_default();
                self.ask("Rename board", &title, "Board name", false, AskFor::RenameTeam(id.clone()));
            }
            Act::DeleteTeam(id) => {
                let title = self.team_boards.iter().find(|b| &b.id == id).map(|b| b.title.clone()).unwrap_or_default();
                self.show_overlay(Overlay::Confirm {
                    title: format!("Delete “{title}”?"),
                    body: "The board is removed for everyone it is shared with. This cannot be undone.".into(),
                    cancel: "Cancel".into(),
                    ok: "Delete".into(),
                    purpose: ConfirmFor::DeleteTeam(id.clone()),
                });
            }
            Act::AddPerson => self.ask("Invite by email", "", "name@company.com", false, AskFor::AddPerson),
            Act::MemberMenu(email) => self.menu(MenuKind::Member(email.clone()), act),
            Act::SetRole(email, role) => {
                self.dismiss();
                self.invite(email.clone(), *role);
            }
            Act::RemoveMember(email) => {
                self.dismiss();
                self.show_overlay(Overlay::Confirm {
                    title: format!("Remove {email}?"),
                    body: "They will no longer be able to open this board.".into(),
                    cancel: "Cancel".into(),
                    ok: "Remove".into(),
                    purpose: ConfirmFor::RemoveMember(email.clone()),
                });
            }
            Act::OrgAccess(level) => {
                let Some(id) = self.live.as_ref().map(|l| l.cloud_id.clone()) else { return true };
                let level = level.to_string();
                self.request(move |a| async move { Reply::Board(id.clone(), cloud::update_board(&a, &id, json!({ "orgAccess": level })).await) });
            }
            _ => return false,
        }
        true
    }

    pub(super) fn invite(&mut self, email: String, role: Role) {
        let Some(id) = self.live.as_ref().map(|l| l.cloud_id.clone()) else { return };
        self.request(move |a| async move { Reply::Members(id.clone(), cloud::add_member(&a, &id, &email, role).await) });
    }

    pub(super) fn remove_member(&mut self, email: String) {
        let Some(id) = self.live.as_ref().map(|l| l.cloud_id.clone()) else { return };
        self.request(move |a| async move { Reply::Members(id.clone(), cloud::remove_member(&a, &id, &email).await) });
    }

    /// A new board in the signed-in person's account, opened as soon as the
    /// server has made it. No name is asked for: it is renamed later.
    pub(super) fn create_team_board(&mut self, title: String) {
        if self.new_board_at.is_some_and(|t| self.now < t + 2000) {
            return;
        }
        self.new_board_at = Some(self.now);
        self.show_toast("Creating your board…");
        self.request(move |a| async move { Reply::Created(cloud::create_board(&a, &title).await) });
    }

    pub(super) fn rename_team(&mut self, id: String, title: String) {
        self.request(move |a| async move { Reply::Board(id.clone(), cloud::update_board(&a, &id, json!({ "title": title })).await) });
    }

    pub(super) fn delete_team(&mut self, id: String) {
        self.request(move |a| async move { Reply::Deleted(id.clone(), cloud::delete_board(&a, &id).await) });
    }

    /// A team board renamed from inside it goes to the server too.
    pub(super) fn rename_live(&mut self, title: &str) -> bool {
        let Some(live) = &self.live else { return false };
        if !live.role.can_edit() {
            self.error_toast("You can view this board but not rename it");
            return true;
        }
        self.rename_team(live.cloud_id.clone(), title.to_string());
        false
    }

    pub(super) fn can_change_pages(&self) -> bool {
        self.live.as_ref().map_or(true, |l| l.role.can_edit())
    }

    pub(super) fn after_page_change(&mut self, _change: &PageChange) {
        self.push_local();
    }

    pub(super) fn new_page_key() -> String {
        random_token(10)
    }
}

/// Draws the team looks; returns false for others.
pub(super) fn draw_look(p: &mut Painter, look: &Look, r: Rf, u: f32) -> bool {
    match look {
        Look::Cursor { color, name } => {
            let (x, y) = (r.x, r.y);
            let s = u;
            let arrow = [(x, y), (x, y + 30.0 * s), (x + 8.0 * s, y + 23.0 * s), (x + 14.0 * s, y + 35.0 * s), (x + 19.0 * s, y + 33.0 * s), (x + 13.0 * s, y + 21.0 * s), (x + 23.0 * s, y + 21.0 * s)];
            let outline: Vec<(f32, f32)> = arrow.iter().map(|(ax, ay)| (*ax, *ay)).collect();
            p.polygon(&outline, 0xffffffff);
            let inner: Vec<(f32, f32)> = arrow.iter().map(|(ax, ay)| (x + (ax - x) * 0.82 + 1.6 * s, y + (ay - y) * 0.82 + 2.4 * s)).collect();
            p.polygon(&inner, *color);
            let label = Rf::new(x + 18.0 * s, y + 30.0 * s, r.w - 18.0 * s, 32.0 * s);
            p.rrect(label, 16.0 * s, *color);
            p.text(Face::Medium, 20.0 * s, name, label.x + 14.0 * s, super::baseline(label, Face::Medium, 20.0 * s), 0xffffffff);
            true
        }
        Look::Outline { color, label, below } => {
            let tag = 30.0 * u;
            let body = if *below { Rf::new(r.x, r.y, r.w, r.h - tag) } else { Rf::new(r.x, r.y + tag, r.w, r.h - tag) };
            p.rrect_line(body, 8.0 * u, *color, 3.0 * u);
            let tw = font::measure(Face::Medium, 18.0 * u, label).0 + 20.0 * u;
            let ty = if *below { body.bottom() + 2.0 * u } else { r.y };
            let t = Rf::new(r.x, ty, tw, tag - 2.0 * u);
            p.rrect(t, 8.0 * u, *color);
            p.text(Face::Medium, 18.0 * u, label, t.x + 10.0 * u, super::baseline(t, Face::Medium, 18.0 * u), 0xffffffff);
            true
        }
        Look::Badge { access } => {
            let (bg, fg, border) = match access {
                Access::Private => (SURFACE, TEXT2, true),
                Access::Shared => (ACCENT_TINT, ACCENT, false),
                Access::Org => (0xffdff3e8, OK, false),
            };
            p.rrect(r, r.h / 2.0, bg);
            if border {
                p.rrect_line(r, r.h / 2.0, BORDER, 1.5 * u);
            }
            p.ico(access.icon(), 22.0 * u, r.x + 25.0 * u, r.cy(), fg);
            let text = Rf::new(r.x + 44.0 * u, r.y, (r.w - 58.0 * u).max(0.0), r.h);
            super::line_text(p, text, Face::Bold, 20.0 * u, &access.label(), fg, false);
            true
        }
        Look::Person { name, email, role, menu } => {
            let side = (r.h - 20.0 * u).min(60.0 * u);
            let color = person_color(email);
            let (cx, cy) = (r.x + side / 2.0, r.cy());
            p.circle(cx, cy, side / 2.0, color);
            let initials = cloud::initials(name);
            let size = side * 0.38;
            let w = font::measure(Face::Bold, size, &initials).0;
            let box_ = Rf::new(cx - side / 2.0, cy - side / 2.0, side, side);
            p.text(Face::Bold, size, &initials, cx - w / 2.0, super::baseline(box_, Face::Bold, size), 0xffffffff);
            // The role, as a pill with an arrow when it can be changed.
            let mut right = r.right();
            if !role.is_empty() {
                let tw = font::measure(Face::Medium, 22.0 * u, role).0;
                let pw = tw + if *menu { 62.0 } else { 36.0 } * u;
                let pill = Rf::new(right - pw, r.cy() - 26.0 * u, pw, 52.0 * u);
                if *menu {
                    p.rrect(pill, pill.h / 2.0, SURFACE2);
                }
                super::line_text(p, Rf::new(pill.x + 18.0 * u, pill.y, tw + 2.0 * u, pill.h), Face::Medium, 22.0 * u, role, TEXT2, false);
                if *menu {
                    let (ax, ay) = (pill.right() - 24.0 * u, pill.cy());
                    p.polygon(&[(ax - 7.0 * u, ay - 3.0 * u), (ax + 7.0 * u, ay - 3.0 * u), (ax, ay + 5.0 * u)], TEXT2);
                }
                right = pill.x - 12.0 * u;
            }
            let x = r.x + side + 18.0 * u;
            let w = (right - x).max(0.0);
            if name == email {
                super::line_text(p, Rf::new(x, r.cy() - 17.0 * u, w, 34.0 * u), Face::Bold, 24.0 * u, name, TEXT1, false);
            } else {
                super::line_text(p, Rf::new(x, r.cy() - 32.0 * u, w, 34.0 * u), Face::Bold, 24.0 * u, name, TEXT1, false);
                super::line_text(p, Rf::new(x, r.cy() + 2.0 * u, w, 30.0 * u), Face::Regular, 20.0 * u, email, TEXT3, false);
            }
            true
        }
        Look::Avatar { text, color } => {
            let side = r.w.min(r.h);
            let (cx, cy) = (r.x + side / 2.0, r.cy());
            p.circle(cx, cy, side / 2.0, 0xffffffff);
            p.circle(cx, cy, side / 2.0 - 2.5 * u, *color);
            let size = side * 0.38;
            let w = font::measure(Face::Bold, size, text).0;
            let box_ = Rf::new(cx - side / 2.0, r.y + (r.h - side) / 2.0, side, side);
            p.text(Face::Bold, size, text, cx - w / 2.0, super::baseline(box_, Face::Bold, size), 0xffffffff);
            true
        }
        _ => false,
    }
}

/// A board menu for team cards and people.
pub(super) fn menu_rows(app: &App, kind: &MenuKind) -> Option<(f32, Vec<Option<(Look, Act)>>)> {
    let row = |icon: Option<Ico>, label: &str, danger: bool, act: Act| Some((Look::Row { icon, text: label.into(), value: String::new(), chevron: false, danger }, act));
    match kind {
        MenuKind::TeamCard(id) => {
            let role = app.team_boards.iter().find(|b| &b.id == id).map_or(Role::Viewer, |b| b.role);
            let mut rows = vec![row(Some(Ico::Page), "Open", false, Act::OpenTeam(id.clone()))];
            if role.can_edit() {
                rows.push(row(Some(Ico::Rename), "Rename", false, Act::RenameTeam(id.clone())));
            }
            rows.push(row(Some(Ico::Share), "Share…", false, Act::ShareTeam(id.clone())));
            if role == Role::Owner {
                rows.push(None);
                rows.push(row(Some(Ico::Trash), "Delete", true, Act::DeleteTeam(id.clone())));
            }
            Some((400.0, rows))
        }
        MenuKind::Account => {
            let mut rows = vec![
                row(Some(Ico::Gear), "Settings", false, Act::Settings),
                row(Some(Ico::Hand), "Phone pad", false, Act::Remote),
                row(Some(Ico::Claude), "Claude", false, Act::Claude),
            ];
            if let Some((version, _)) = &app.update {
                rows.push(Some((
                    Look::Row { icon: Some(Ico::Update), text: "Update".into(), value: format!("{version} ready"), chevron: false, danger: false },
                    Act::Updates,
                )));
            }
            rows.push(None);
            rows.push(row(Some(Ico::Exit), "Sign out", false, Act::SignOut));
            Some((560.0, rows))
        }
        MenuKind::Member(email) => Some((
            360.0,
            vec![
                row(None, "Can edit", false, Act::SetRole(email.clone(), Role::Editor)),
                row(None, "Can view", false, Act::SetRole(email.clone(), Role::Viewer)),
                None,
                row(Some(Ico::Trash), "Remove", true, Act::RemoveMember(email.clone())),
            ],
        )),
        _ => None,
    }
}

/// What the library shows for a team board.
pub(super) struct TeamCard<'a> {
    pub board: &'a CloudBoard,
    /// "Edited 5 min ago", with the owner's name first on boards shared with you.
    pub subtitle: String,
    pub access: Access,
}

/// The boards the current filter keeps, most recently changed first.
pub(super) fn team_cards(app: &App) -> Vec<TeamCard<'_>> {
    let mut boards: Vec<&CloudBoard> = app.team_boards.iter().filter(|b| app.tab.keeps(b)).collect();
    boards.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    boards
        .into_iter()
        .map(|b| {
            let edited = if b.updated_at > 0 { format!("Edited {}", ago(b.updated_at)) } else { "New".to_string() };
            let subtitle = if b.role == Role::Owner { edited } else { format!("{}  ·  {edited}", person_name(&b.owner_name, &b.owner_email)) };
            TeamCard { board: b, subtitle, access: app.access_of(b) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Two apps on one team board, against a running server (see
    //! `cloud/live_test.rs` for the variables). Skipped when they are unset.
    use super::super::{App, Platform};
    use super::*;
    use crate::shape::Tool;
    use std::time::{Duration, Instant};

    struct NoHost;
    impl Platform for NoHost {}

    fn signed_in(server: &str, token: &str) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("inkboard-team-{}-{}", std::process::id(), random_token(8)));
        let mut app = App::new(Box::new(NoHost), dir.join("boards"), dir.join("settings.json"), "0.9.0".into(), 0, Arc::new(|| {}));
        let me = cloud::User::from_json(&cloud::http::api_blocking(server, "GET", "/api/me", Some(token), None).unwrap()).unwrap();
        app.settings.server = server.to_string();
        app.settings.account = Some(Account { server: server.to_string(), token: token.to_string(), user: me });
        app.resize(1280, 800, 1.5);
        (app, dir)
    }

    fn pump(apps: &mut [&mut App], t: &mut u64, done: impl Fn(&[&mut App]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            for app in apps.iter_mut() {
                *t += 20;
                app.frame(*t);
            }
            if done(apps) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out");
    }

    /// Saves what `app` shows when INKBOARD_SNAPSHOT_DIR is set.
    fn snapshot(app: &App, name: &str) {
        let Ok(dir) = std::env::var("INKBOARD_SNAPSHOT_DIR") else { return };
        let out = app.output();
        let mut data = out.data.clone();
        for px in data.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let size = tiny_skia::IntSize::from_wh(out.w, out.h).unwrap();
        let png = tiny_skia::Pixmap::from_vec(data, size).unwrap().encode_png().unwrap();
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), png).unwrap();
    }

    fn live_ready(app: &App) -> bool {
        app.live.as_ref().is_some_and(|l| l.synced && l.online == Some(true))
    }

    #[test]
    fn sign_in_shows_the_code_to_enter() {
        let dir = std::env::temp_dir().join(format!("inkboard-signin-{}-{}", std::process::id(), random_token(6)));
        let mut app = App::new(Box::new(NoHost), dir.join("boards"), dir.join("settings.json"), "0.9.0".into(), 0, Arc::new(|| {}));
        app.resize(1280, 800, 1.5);
        let nodes = app.layout();
        assert!(nodes.iter().any(|n| n.act == Some(Act::SignIn)));
        app.panel(PanelKind::Account);
        app.sign_in = Some(SignIn { cancel: Arc::new(AtomicBool::new(false)), code: Some(("WXYZ-QRST".into(), "https://www.google.com/device".into())), error: None, browser: false });
        let nodes = app.layout();
        assert!(nodes.iter().any(|n| matches!(&n.look, Look::Label { text, .. } if text == "WXYZ-QRST")));
        assert!(nodes.iter().any(|n| matches!(&n.look, Look::Qr(url) if url == "https://www.google.com/device")));
        app.frame(2000);
        snapshot(&app, "sign-in-code");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_apps_share_a_team_board() {
        let (Ok(server), Ok(ta), Ok(tb)) = (
            std::env::var("INKBOARD_TEST_SERVER"),
            std::env::var("INKBOARD_TEST_TOKEN_A"),
            std::env::var("INKBOARD_TEST_TOKEN_B"),
        ) else {
            return;
        };
        let (mut ana, dir_a) = signed_in(&server, &ta);
        let (mut ben, dir_b) = signed_in(&server, &tb);
        let mut t = 1000;

        ana.create_team_board("Pairing".into());
        pump(&mut [&mut ana], &mut t, |a| live_ready(a[0]));
        let cloud_id = ana.live.as_ref().unwrap().cloud_id.clone();
        assert_eq!(ana.live.as_ref().unwrap().role, Role::Owner);
        let account = ana.account().unwrap().clone();
        cloud::block_on(cloud::add_member(&account, &cloud_id, &ben.me().unwrap().email, Role::Editor)).unwrap();

        pump(&mut [&mut ben], &mut t, |a| a[0].team_boards.iter().any(|b| b.id == cloud_id));
        assert!(ben.cards().iter().any(|c| c.title == "Pairing"));
        ben.run(Act::OpenTeam(cloud_id.clone()));
        pump(&mut [&mut ana, &mut ben], &mut t, |a| live_ready(a[1]) && !a[0].live.as_ref().unwrap().peers.is_empty());

        // Ana draws; Ben gets it, credited to her.
        let rect = Shape::new("r1", Tool::Rectangle, vec![Pt::new(10.0, 10.0), Pt::new(200.0, 120.0)]);
        ana.ed.apply_shapes(vec![rect], Vec::new());
        ana.take_editor_events();
        pump(&mut [&mut ana, &mut ben], &mut t, |a| a[1].ed.doc.contains("r1"));
        ben.ed.doc.select(Some("r1"));
        let line = ben.attribution_line().unwrap();
        assert!(line.starts_with(&format!("Added by {}", ana.me().unwrap().first_name())), "{line}");
        // Ben's undo does not take back Ana's shape.
        ben.ed.undo();
        assert!(ben.ed.doc.contains("r1"));

        // Ana's pointer shows on Ben's screen, and her board changes flash there.
        ana.hover(300.0, 200.0);
        pump(&mut [&mut ana, &mut ben], &mut t, |a| a[1].live.as_ref().unwrap().peers.values().any(|p| p.cursor.is_some()));
        ana.ed.doc.select(Some("r1"));
        ana.send_presence();
        pump(&mut [&mut ana, &mut ben], &mut t, |a| a[1].live.as_ref().unwrap().peers.values().any(|p| !p.selection.is_empty()));
        ben.ed.doc.select(Some("r1"));
        ben.frame(t + 20);
        snapshot(&ben, "ben-sees-ana");
        ben.ed.doc.select(None);
        ana.ed.doc.select(None);
        let nodes = ben.layout();
        assert!(nodes.iter().any(|n| matches!(&n.look, Look::Cursor { name, .. } if name == ana.me().unwrap().first_name())));

        // Ben moves it; Ana sees the move, now edited by Ben.
        let moved = ben.ed.doc.get("r1").unwrap().with_points(vec![Pt::new(50.0, 50.0), Pt::new(240.0, 160.0)]);
        ben.ed.apply_shapes(vec![moved], Vec::new());
        ben.take_editor_events();
        pump(&mut [&mut ana, &mut ben], &mut t, |a| a[0].ed.doc.get("r1").is_some_and(|s| s.points[0].x == 50.0));
        ana.ed.doc.select(Some("r1"));
        assert!(ana.attribution_line().unwrap().contains(&format!("edited by {}", ben.me().unwrap().first_name())));

        ana.frame(t + 20);
        snapshot(&ana, "ana-after-ben-moved");
        ana.run(Act::Share);
        pump(&mut [&mut ana], &mut t, |a| a[0].members.is_some());
        ana.frame(t + 20);
        snapshot(&ana, "ana-share-panel");
        ana.dismiss();
        // A new page reaches Ben too.
        ana.change_page(PageChange::Add);
        pump(&mut [&mut ana, &mut ben], &mut t, |a| a[1].pages.len() == 2);

        // Ben deletes the rectangle on page one.
        ben.ed.delete_shapes(vec!["r1".into()]);
        ben.take_editor_events();
        pump(&mut [&mut ana, &mut ben], &mut t, |a| !a[0].pages[0].shapes.iter().any(|s| s.id == "r1"));

        ana.close_board();
        pump(&mut [&mut ana], &mut t, |a| a[0].board.is_none() && a[0].team_listed);
        ana.frame(t + 20);
        snapshot(&ana, "ana-library");
        cloud::block_on(cloud::delete_board(&account, &cloud_id)).unwrap();
        let _ = std::fs::remove_dir_all(dir_a);
        let _ = std::fs::remove_dir_all(dir_b);
    }

    // ------------------------------------------------- library, no server

    use crate::store;
    use std::sync::Mutex;

    /// A host that remembers what was copied.
    struct Clip(Arc<Mutex<Vec<String>>>);
    impl Platform for Clip {
        fn copy_text(&mut self, text: &str) {
            self.0.lock().unwrap().push(text.to_string());
        }
    }

    fn offline_app(copied: Arc<Mutex<Vec<String>>>) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("inkboard-lib-{}-{}", std::process::id(), random_token(8)));
        let mut app = App::new(Box::new(Clip(copied)), dir.join("boards"), dir.join("settings.json"), "0.9.0".into(), 0, Arc::new(|| {}));
        // Nothing listens here, so requests fail at once.
        app.settings.server = "http://127.0.0.1:1".into();
        (app, dir)
    }

    fn sign_in_here(app: &mut App) {
        let user = cloud::User { id: "u1".into(), email: "shashank@kenpath.com".into(), name: "Shashank Rao".into(), color: 0xff2748e8 };
        app.settings.account = Some(Account { server: app.settings.server.clone(), token: "t".into(), user });
    }

    fn board(id: &str, title: &str, role: Role, org: &str, owner: &str, minutes_ago: i64) -> CloudBoard {
        CloudBoard {
            id: id.into(),
            title: title.into(),
            role,
            org_access: org.into(),
            owner_name: owner.into(),
            owner_email: format!("{}@kenpath.com", owner.to_lowercase().replace(' ', ".")),
            updated_at: super::super::wall_millis() - minutes_ago * 60_000,
            members: 0,
        }
    }

    fn some_boards() -> Vec<CloudBoard> {
        vec![
            board("a", "Sprint planning", Role::Owner, "none", "Shashank Rao", 5),
            board("b", "Kenpath roadmap", Role::Owner, "view", "Shashank Rao", 90),
            board("c", "Brand sketches", Role::Editor, "none", "Ana Iyer", 600),
            board("d", "Onsite map", Role::Viewer, "edit", "Ben Thomas", 3000),
        ]
    }

    fn titles(app: &App) -> Vec<String> {
        app.cards().into_iter().map(|c| c.title).collect()
    }

    #[test]
    fn signed_out_the_library_is_a_welcome_screen() {
        let (mut app, dir) = offline_app(Default::default());
        app.resize(1920, 1080, 1.5);
        settle_frames(&mut app);
        let nodes = app.layout();
        let sign_in = nodes.iter().find(|n| n.act == Some(Act::SignIn)).expect("a sign-in button");
        assert!(matches!(&sign_in.look, Look::Button { text, .. } if text == "Sign in with Google"));
        assert!(nodes.iter().any(|n| matches!(&n.look, Look::Label { text, .. } if text.contains("Kenpath Google Workspace"))));
        // No boards, tabs or New board button before signing in.
        assert!(!nodes.iter().any(|n| n.act == Some(Act::NewBoard)));
        assert!(!nodes.iter().any(|n| matches!(n.act, Some(Act::Filter(_)) | Some(Act::OpenTeam(_)) | Some(Act::Open(_)))));
        assert!(!nodes.iter().any(|n| matches!(n.look, Look::Thumb { .. })));
        // Nor the old header pills.
        assert!(!nodes.iter().any(|n| matches!(n.act, Some(Act::Remote) | Some(Act::Claude) | Some(Act::Account))));
        // Phones get the same screen.
        app.resize(720, 1280, 2.0);
        assert!(app.layout().iter().any(|n| n.act == Some(Act::SignIn)));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn settle_frames(app: &mut App) {
        for i in 0..20 {
            app.frame(1000 + i);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn signed_in_the_header_is_an_avatar_with_an_account_menu() {
        let (mut app, dir) = offline_app(Default::default());
        sign_in_here(&mut app);
        app.team_boards = some_boards();
        app.team_listed = true;
        app.resize(1920, 1080, 1.5);
        settle_frames(&mut app);
        let nodes = app.layout();
        let avatar = nodes.iter().find(|n| n.act == Some(Act::AccountMenu)).expect("the avatar");
        assert!(matches!(&avatar.look, Look::Avatar { text, .. } if text == "SR"));
        for gone in [Act::Remote, Act::Claude, Act::Settings, Act::Account, Act::SignIn] {
            assert!(!nodes.iter().any(|n| n.act == Some(gone.clone())), "{gone:?} is in the menu now");
        }
        app.run(Act::AccountMenu);
        app.frame(2000);
        let acts: Vec<Act> = app.nodes.iter().filter_map(|n| n.act.clone()).collect();
        for want in [Act::Settings, Act::Remote, Act::Claude, Act::SignOut] {
            assert!(acts.contains(&want), "{want:?} missing from the account menu");
        }
        assert!(app.nodes.iter().any(|n| matches!(&n.look, Look::Label { text, .. } if text == "shashank@kenpath.com")));
        // Update stays reachable from the menu when one is ready.
        app.update = Some(("1.0.0".into(), String::new()));
        app.frame(2100);
        assert!(app.nodes.iter().any(|n| n.act == Some(Act::Updates)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn filters_choose_the_right_cards_and_badges() {
        let (mut app, dir) = offline_app(Default::default());
        sign_in_here(&mut app);
        app.team_boards = some_boards();
        app.team_listed = true;
        app.resize(1920, 1080, 1.5);
        // Most recently edited first.
        assert_eq!(titles(&app), ["Sprint planning", "Kenpath roadmap", "Brand sketches", "Onsite map"]);
        app.run(Act::Filter(Tab::Yours));
        assert_eq!(titles(&app), ["Sprint planning", "Kenpath roadmap"]);
        app.run(Act::Filter(Tab::Shared));
        assert_eq!(titles(&app), ["Brand sketches", "Onsite map"]);
        // Shared-with-you cards name the owner; yours say when they changed.
        let cards = app.cards();
        assert!(cards[0].subtitle.starts_with("Ana Iyer"), "{}", cards[0].subtitle);
        assert!(cards[0].subtitle.contains("Edited 10 h ago"), "{}", cards[0].subtitle);
        app.run(Act::Filter(Tab::All));
        assert_eq!(app.cards()[0].subtitle, "Edited 5 min ago");
        // Badges: private, org-wide, shared with me, org-wide.
        let access: Vec<Access> = app.cards().iter().map(|c| c.access).collect();
        assert_eq!(access, [Access::Private, Access::Org, Access::Shared, Access::Org]);
        let badges: Vec<Access> = app.layout().iter().filter_map(|n| if let Look::Badge { access } = n.look { Some(access) } else { None }).collect();
        assert_eq!(badges, access);
        // Inviting someone makes a private board shared.
        app.invited.insert("a".into());
        assert_eq!(app.cards()[0].access, Access::Shared);
        // A filter with nothing in it says so instead of showing an empty grid.
        app.team_boards.retain(|b| b.role == Role::Owner);
        app.run(Act::Filter(Tab::Shared));
        assert!(app.cards().is_empty());
        assert!(app.layout().iter().any(|n| matches!(&n.look, Look::Label { text, .. } if text.starts_with("Nothing shared"))));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_empty_account_invites_a_first_board() {
        let (mut app, dir) = offline_app(Default::default());
        sign_in_here(&mut app);
        app.team_listed = true;
        app.resize(1920, 1080, 1.5);
        let nodes = app.layout();
        assert!(nodes.iter().any(|n| matches!(&n.look, Look::Label { text, .. } if text == "Create your first board")));
        assert_eq!(nodes.iter().filter(|n| n.act == Some(Act::NewBoard)).count(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn new_board_signed_in_skips_the_name_dialog() {
        let (mut app, dir) = offline_app(Default::default());
        sign_in_here(&mut app);
        app.run(Act::NewBoard);
        assert!(app.overlay.is_none(), "no dialog while signed in");
        assert!(app.new_board_at.is_some(), "the board is being made");
        // A second tap right away does not make another.
        let first = app.new_board_at;
        app.run(Act::NewBoard);
        assert_eq!(app.new_board_at, first);
        // Signed out, a board on this device is made at once too.
        let (mut local, dir2) = offline_app(Default::default());
        local.run(Act::NewBoard);
        assert!(local.overlay.is_none(), "no dialog signed out either");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }

    #[test]
    fn card_menu_offers_open_rename_share_and_delete() {
        let (mut app, dir) = offline_app(Default::default());
        sign_in_here(&mut app);
        app.team_boards = some_boards();
        let acts = |app: &App, id: &str| -> Vec<Act> {
            menu_rows(app, &MenuKind::TeamCard(id.into())).unwrap().1.into_iter().flatten().map(|(_, a)| a).collect()
        };
        assert_eq!(
            acts(&app, "a"),
            [Act::OpenTeam("a".into()), Act::RenameTeam("a".into()), Act::ShareTeam("a".into()), Act::DeleteTeam("a".into())]
        );
        // An editor cannot delete; a viewer cannot rename either.
        assert_eq!(acts(&app, "c"), [Act::OpenTeam("c".into()), Act::RenameTeam("c".into()), Act::ShareTeam("c".into())]);
        assert_eq!(acts(&app, "d"), [Act::OpenTeam("d".into()), Act::ShareTeam("d".into())]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn share_copies_the_board_link() {
        let copied: Arc<Mutex<Vec<String>>> = Default::default();
        let (mut app, dir) = offline_app(copied.clone());
        app.settings.server = "https://ink.example/".into();
        app.run(Act::CopyBoardLink("abc123".into()));
        assert_eq!(*copied.lock().unwrap(), ["https://ink.example/app/#board=abc123"]);
        assert_eq!(app.toast.as_ref().unwrap().text, "Link copied");
        // Signed out, the panel offers sign-in rather than copying to Team boards.
        let (rows, _) = app.panel_rows(PanelKind::Share, 700.0);
        assert!(rows.iter().any(|(_, look, act)| *act == Some(Act::SignIn) && matches!(look, Look::Button { text, .. } if text == "Sign in to share")));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Writes pictures of the new screens when INKBOARD_SNAPSHOT_DIR is set.
    #[test]
    fn library_screenshots() {
        if std::env::var("INKBOARD_SNAPSHOT_DIR").is_err() {
            return;
        }
        let (mut app, dir) = offline_app(Default::default());
        let sizes = [("wide", 1920, 1080, 1.5), ("phone", 1080, 2340, 3.0)];
        for (name, w, h, d) in sizes {
            app.resize(w, h, d);
            app.frame(1000);
            snapshot(&app, &format!("{name}-1-signed-out"));
        }
        // Boards with pictures: the cached copy of each team board.
        sign_in_here(&mut app);
        app.team_boards = some_boards();
        app.team_listed = true;
        for (i, b) in app.team_boards.clone().iter().enumerate() {
            let id = cache_id(&b.id);
            let shapes: Vec<Arc<Shape>> = (0..4 + i)
                .map(|k| Arc::new(Shape::new(&format!("s{k}"), Tool::Rectangle, vec![Pt::new(40.0 + 90.0 * k as f64, 60.0 + 40.0 * (k % 2) as f64), Pt::new(110.0 + 90.0 * k as f64, 130.0 + 40.0 * (k % 2) as f64)])))
                .collect();
            let contents = store::encode(&id, &b.title, "2026-11-19T10:00:00.000", &shapes, &Map::new());
            store::save_atomic(&dir.join("boards"), &id, &contents).unwrap();
        }
        app.send(Job::List);
        for (name, w, h, d) in sizes {
            app.resize(w, h, d);
            for i in 0..40 {
                app.frame(2000 + i * 20);
                std::thread::sleep(Duration::from_millis(10));
            }
            snapshot(&app, &format!("{name}-2-boards"));
            app.run(Act::Filter(Tab::Shared));
            app.frame(4000);
            snapshot(&app, &format!("{name}-3-shared-with-you"));
            app.run(Act::Filter(Tab::All));
            app.run(Act::AccountMenu);
            app.frame(4100);
            app.frame(4400);
            snapshot(&app, &format!("{name}-4-account-menu"));
            app.dismiss();
            app.run(Act::TeamMenu("a".into()));
            app.frame(4500);
            app.frame(4800);
            snapshot(&app, &format!("{name}-5-card-menu"));
            app.dismiss();
        }
        app.update = Some(("1.0.0".into(), String::new()));
        app.frame(5000);
        snapshot(&app, "phone-6-update");
        app.update = None;
        // The share panel of an open team board.
        let mut extra = Map::new();
        extra.insert("cloud".into(), json!({ "id": "a", "role": "owner" }));
        let contents = store::encode(&cache_id("a"), "Sprint planning", "2026-11-19T10:00:00.000", &[], &extra);
        store::save_atomic(&dir.join("boards"), &cache_id("a"), &contents).unwrap();
        app.run(Act::OpenTeam("a".into()));
        for i in 0..30 {
            app.frame(6000 + i * 20);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(app.live.is_some(), "board did not open");
        let person = |email: &str, name: &str, role| cloud::Member { email: email.into(), name: name.into(), role };
        app.members = Some((
            "a".into(),
            cloud::Members {
                org_access: "none".into(),
                owner: Some(person("shashank@kenpath.com", "Shashank Rao", Role::Owner)),
                members: vec![person("ana.iyer@kenpath.com", "Ana Iyer", Role::Editor), person("ben@kenpath.com", "", Role::Viewer)],
            },
        ));
        for (name, w, h, d) in sizes {
            app.resize(w, h, d);
            app.panel(PanelKind::Share);
            app.frame(7000);
            app.frame(7300);
            snapshot(&app, &format!("{name}-7-share"));
            app.dismiss();
            app.members.as_mut().unwrap().1.org_access = "view".into();
            app.panel(PanelKind::Share);
            app.frame(7400);
            snapshot(&app, &format!("{name}-8-share-org"));
            app.dismiss();
            app.members.as_mut().unwrap().1.org_access = "none".into();
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
