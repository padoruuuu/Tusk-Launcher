//! SNI (StatusNotifierItem) host + watcher + DBusMenu implementation.
//!
//! Design:
//! - Claims `org.kde.StatusNotifierWatcher` so new apps register directly.
//! - Queries existing watchers for already-registered items on startup.
//! - Subscribes to `StatusNotifierItemRegistered` signals from all watchers.
//! - Per-item signal tasks refresh icons on `NewIcon` / `NewStatus` / etc.
//! - Items removed when their bus name vanishes.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use futures_util::StreamExt;
use zbus::{interface, Connection};
use zbus::connection::Builder as ConnectionBuilder;
use crate::gui::Config;

// ============================================================================
// Constants
// ============================================================================

const T_PROBE: Duration = Duration::from_secs(2);
const T_FETCH: Duration = Duration::from_secs(5);

const SNI_INTERFACES: &[&str] = &[
    "org.kde.StatusNotifierItem",
    "org.ayatana.AppIndicator",
    "org.freedesktop.StatusNotifierItem",
];

const SNI_PATHS: &[&str] = &[
    "/StatusNotifierItem",
    "/org/kde/StatusNotifierItem",
];

const WATCHER_NAMES: &[&str] = &[
    "org.kde.StatusNotifierWatcher",
    "org.freedesktop.StatusNotifierWatcher",
];

// ============================================================================
// Public types
// ============================================================================

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TrayCategory {
    #[default] ApplicationStatus,
    Communications,
    SystemServices,
    Hardware,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TrayStatus { #[default] Active, Passive, NeedsAttention }

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ToggleType { #[default] None, Checkmark, Radio }

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct MenuItem {
    pub id:           i32,
    pub label:        String,
    pub enabled:      bool,
    pub visible:      bool,
    pub is_separator: bool,
    pub icon_name:    Option<String>,
    pub toggle_type:  ToggleType,
    pub toggle_state: i32,
    pub children:     Vec<MenuItem>,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct TrayIcon {
    pub id:              String,
    pub bus_name:        String,
    pub obj_path:        String,
    pub category:        TrayCategory,

    pub icon_rgba:       Vec<u8>,
    pub icon_w:          u32,
    pub icon_h:          u32,
    pub icon_name:       Option<String>,
    pub icon_theme_path: Option<String>,

    pub attention_icon_rgba:  Vec<u8>,
    pub attention_icon_w:     u32,
    pub attention_icon_h:     u32,
    pub attention_icon_name:  Option<String>,

    pub overlay_icon_rgba: Vec<u8>,
    pub overlay_icon_w:    u32,
    pub overlay_icon_h:    u32,
    pub overlay_icon_name: Option<String>,

    pub status:       TrayStatus,
    pub item_is_menu: bool,

    pub tooltip_title: String,
    pub tooltip_body:  String,

    pub menu_path:     Option<String>,
    pub menu_items:    Vec<MenuItem>,
    pub menu_revision: u32,
    pub menu_loaded:   bool,

    /// Bumped whenever pixel data changes; GUI uses this to detect stale textures.
    pub icon_rev: u32,
}

pub type TrayItems = Arc<Mutex<Vec<TrayIcon>>>;

#[allow(dead_code)]
pub enum SniAction {
    Activate          { bus_name: String, obj_path: String },
    SecondaryActivate { bus_name: String, obj_path: String },
    ContextMenu       { bus_name: String, obj_path: String, x: i32, y: i32 },
    Scroll            { bus_name: String, obj_path: String, delta: i32, orientation: String },
    MenuAboutToShow   { bus_name: String, menu_path: String },
    MenuEvent         { bus_name: String, menu_path: String, item_id: i32 },
    FetchMenu         { bus_name: String, menu_path: String, service_id: String },
    RefreshMenu       { bus_name: String, menu_path: String, service_id: String },
}

pub struct SniHost {
    pub items:     TrayItems,
    pub action_tx: tokio::sync::mpsc::UnboundedSender<SniAction>,
}

impl SniHost {
    pub fn new(config: &Config) -> Option<Self> {
        if !config.enable_system_tray { return None; }

        let items: TrayItems = Arc::new(Mutex::new(Vec::new()));
        let items_bg = Arc::clone(&items);
        let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel();

        thread::spawn(move || {
            match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt.block_on(async {
                    if let Err(e) = run_watcher(items_bg, action_rx).await {
                        eprintln!("SNI watcher: {e}");
                    }
                }),
                Err(e) => eprintln!("SNI: runtime error: {e}"),
            }
        });

        Some(SniHost { items, action_tx })
    }

    fn send(&self, action: SniAction) { let _ = self.action_tx.send(action); }

    pub fn activate(&self, bus_name: &str, obj_path: &str) {
        self.send(SniAction::Activate { bus_name: bus_name.into(), obj_path: obj_path.into() });
    }

    #[allow(dead_code)]
    pub fn secondary_activate(&self, bus_name: &str, obj_path: &str) {
        self.send(SniAction::SecondaryActivate { bus_name: bus_name.into(), obj_path: obj_path.into() });
    }

    pub fn fetch_menu(&self, bus_name: &str, menu_path: &str, service_id: &str) {
        self.send(SniAction::FetchMenu {
            bus_name: bus_name.into(), menu_path: menu_path.into(), service_id: service_id.into(),
        });
    }

    pub fn scroll(&self, bus_name: &str, obj_path: &str, delta: i32, orientation: &str) {
        self.send(SniAction::Scroll {
            bus_name: bus_name.into(), obj_path: obj_path.into(), delta, orientation: orientation.into(),
        });
    }

    pub fn context_menu(&self, bus_name: &str, obj_path: &str, x: i32, y: i32) {
        self.send(SniAction::ContextMenu { bus_name: bus_name.into(), obj_path: obj_path.into(), x, y });
    }

    pub fn menu_about_to_show(&self, bus_name: &str, menu_path: &str) {
        self.send(SniAction::MenuAboutToShow { bus_name: bus_name.into(), menu_path: menu_path.into() });
    }

    pub fn menu_event(&self, bus_name: &str, menu_path: &str, item_id: i32) {
        self.send(SniAction::MenuEvent {
            bus_name: bus_name.into(), menu_path: menu_path.into(), item_id,
        });
    }
}

// ============================================================================
// D-Bus Watcher interface
// ============================================================================

struct Watcher {
    items:      TrayItems,
    registered: Mutex<Vec<String>>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: String,
        #[zbus(header)]     hdr:  zbus::message::Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) {
        let sender = hdr.sender()
            .map(|s: &zbus::names::UniqueName| s.to_string())
            .unwrap_or_default();

        let full = if service.starts_with('/') {
            format!("{sender}{service}")
        } else if service.is_empty() {
            format!("{sender}/StatusNotifierItem")
        } else if service.contains('/') {
            service
        } else {
            format!("{service}/StatusNotifierItem")
        };

        {
            let mut reg = self.registered.lock().unwrap();
            if reg.contains(&full) { return; }
            reg.push(full.clone());
        }

        {
            let conn2 = conn.clone();
            let full2 = full.clone();
            tokio::spawn(async move {
                if let Ok(ctx) = zbus::object_server::SignalEmitter::new(&conn2, "/StatusNotifierWatcher") {
                    let _ = Watcher::status_notifier_item_registered(&ctx, &full2).await;
                }
            });
        }

        let items = Arc::clone(&self.items);
        let conn  = conn.clone();
        tokio::spawn(async move { fetch_and_watch(&conn, &full, items).await; });
    }

    async fn register_status_notifier_host(&self, _service: String) {}

    #[zbus(property)] fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registered.lock().unwrap().clone()
    }
    #[zbus(property)] fn is_status_notifier_host_registered(&self) -> bool { true }
    #[zbus(property)] fn protocol_version(&self) -> i32 { 0 }

    #[zbus(signal)]
    async fn status_notifier_item_registered(ctxt: &zbus::object_server::SignalEmitter<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(ctxt: &zbus::object_server::SignalEmitter<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(ctxt: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;
}

// ============================================================================
// Watcher startup
// ============================================================================

async fn run_watcher(
    items:         TrayItems,
    mut action_rx: tokio::sync::mpsc::UnboundedReceiver<SniAction>,
) -> zbus::Result<()> {
    let conn = Connection::session().await?;

    let watcher_conn = try_become_watcher(Arc::clone(&items)).await;
    eprintln!("SNI: watcher {}", if watcher_conn.is_some() { "claimed" } else { "not claimed" });

    let host_name = format!("org.kde.StatusNotifierHost-{}", std::process::id());
    let _ = conn.request_name(host_name.as_str()).await;

    if let Some(ref wc) = watcher_conn {
        if let Ok(ctx) = zbus::object_server::SignalEmitter::new(wc, "/StatusNotifierWatcher") {
            let _ = Watcher::status_notifier_host_registered(&ctx).await;
        }
    }

    // Harvest items already registered with any active watcher.
    for wname in WATCHER_NAMES {
        for service in query_watcher_items(&conn, wname).await {
            let c = conn.clone(); let i = Arc::clone(&items);
            tokio::spawn(async move { probe_service(&c, &service, i).await; });
        }
    }

    // Scan all unique bus names for SNI items not registered with any watcher.
    if let Ok(msg) = conn.call_method(
        Some("org.freedesktop.DBus"), "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"), "ListNames", &(),
    ).await {
        let all_names: Vec<String> = msg.body().deserialize().unwrap_or_default();
        for name in all_names.into_iter().filter(|n| n.starts_with(':')) {
            let c = conn.clone(); let i = Arc::clone(&items);
            tokio::spawn(async move { scan_one_bus_name(&c, &name, i).await; });
        }
    }

    // Watch StatusNotifierItemRegistered signals from all active watchers.
    for wname in WATCHER_NAMES {
        let rule = match zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(*wname).ok()
            .and_then(|b| b.interface("org.kde.StatusNotifierWatcher").ok())
            .and_then(|b| b.member("StatusNotifierItemRegistered").ok())
            .map(|b| b.build())
        {
            Some(r) => r,
            None    => continue,
        };
        if let Ok(mut stream) = zbus::MessageStream::for_match_rule(rule, &conn, None).await {
            let c = conn.clone(); let i = Arc::clone(&items);
            tokio::spawn(async move {
                while let Some(Ok(msg)) = stream.next().await {
                    if let Ok(service) = msg.body().deserialize::<String>() {
                        let c2 = c.clone(); let i2 = Arc::clone(&i);
                        tokio::spawn(async move { probe_service(&c2, &service, i2).await; });
                    }
                }
            });
        }
    }

    // Watch for bus names appearing/vanishing.
    {
        let dbus       = zbus::fdo::DBusProxy::new(&conn).await?;
        let mut stream = dbus.receive_name_owner_changed().await?;
        let items_w    = Arc::clone(&items);
        let conn_w     = conn.clone();
        tokio::spawn(async move {
            while let Some(sig) = stream.next().await {
                let Ok(args) = sig.args() else { continue };
                let name = args.name().to_string();
                if args.new_owner().is_some() {
                    if name.starts_with(':') {
                        let c = conn_w.clone(); let i = Arc::clone(&items_w);
                        tokio::spawn(async move { scan_one_bus_name(&c, &name, i).await; });
                    }
                } else {
                    let prefix = format!("{name}/");
                    items_w.lock().unwrap().retain(|i| i.bus_name != name && !i.id.starts_with(&prefix));
                }
            }
        });
    }

    // Action handler.
    let conn_act  = conn.clone();
    let items_act = Arc::clone(&items);
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            handle_action(&conn_act, action, Arc::clone(&items_act)).await;
        }
    });

    loop { tokio::time::sleep(Duration::from_secs(60)).await; }
}

async fn handle_action(conn: &Connection, action: SniAction, items: TrayItems) {
    match action {
        SniAction::Activate { bus_name, obj_path } => {
            let _ = conn.call_method(
                Some(bus_name.as_str()), obj_path.as_str(),
                Some("org.kde.StatusNotifierItem"), "Activate", &(0i32, 0i32),
            ).await;
        }
        SniAction::SecondaryActivate { bus_name, obj_path } => {
            let _ = conn.call_method(
                Some(bus_name.as_str()), obj_path.as_str(),
                Some("org.kde.StatusNotifierItem"), "SecondaryActivate", &(0i32, 0i32),
            ).await;
        }
        SniAction::ContextMenu { bus_name, obj_path, x, y } => {
            let _ = conn.call_method(
                Some(bus_name.as_str()), obj_path.as_str(),
                Some("org.kde.StatusNotifierItem"), "ContextMenu", &(x, y),
            ).await;
        }
        SniAction::Scroll { bus_name, obj_path, delta, orientation } => {
            let _ = conn.call_method(
                Some(bus_name.as_str()), obj_path.as_str(),
                Some("org.kde.StatusNotifierItem"), "Scroll", &(delta, orientation.as_str()),
            ).await;
        }
        SniAction::MenuAboutToShow { bus_name, menu_path } => {
            let _ = conn.call_method(
                Some(bus_name.as_str()), menu_path.as_str(),
                Some("com.canonical.dbusmenu"), "AboutToShow", &(0i32,),
            ).await;
        }
        SniAction::MenuEvent { bus_name, menu_path, item_id } => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default().as_secs() as u32;
            let data = zbus::zvariant::Value::I32(0);
            let _ = conn.call_method(
                Some(bus_name.as_str()), menu_path.as_str(),
                Some("com.canonical.dbusmenu"), "Event",
                &(item_id, "clicked", &data, ts),
            ).await;
        }
        SniAction::FetchMenu { bus_name, menu_path, service_id }
        | SniAction::RefreshMenu { bus_name, menu_path, service_id } => {
            let items2 = Arc::clone(&items);
            let conn2  = conn.clone();
            tokio::spawn(async move {
                fetch_menu_internal(&conn2, &bus_name, &menu_path, &service_id, items2).await;
            });
        }
    }
}

// ============================================================================
// Watcher helpers
// ============================================================================

async fn try_become_watcher(items: TrayItems) -> Option<Connection> {
    let watcher = Watcher { items, registered: Mutex::new(Vec::new()) };
    match tokio::time::timeout(T_FETCH, async {
        ConnectionBuilder::session()?
            .name("org.kde.StatusNotifierWatcher")?
            .serve_at("/StatusNotifierWatcher", watcher)?
            .build().await
    }).await {
        Ok(Ok(conn)) => {
            let _ = conn.request_name("org.freedesktop.StatusNotifierWatcher").await;
            Some(conn)
        }
        _ => None,
    }
}

async fn query_watcher_items(conn: &Connection, watcher_name: &str) -> Vec<String> {
    let msg = match tokio::time::timeout(
        Duration::from_secs(3),
        conn.call_method(
            Some(watcher_name), "/StatusNotifierWatcher",
            Some("org.freedesktop.DBus.Properties"), "Get",
            &("org.kde.StatusNotifierWatcher", "RegisteredStatusNotifierItems"),
        ),
    ).await { Ok(Ok(m)) => m, _ => return Vec::new() };

    use zbus::zvariant::Value;
    let outer: zbus::zvariant::OwnedValue = match msg.body().deserialize() {
        Ok(v) => v, Err(_) => return Vec::new(),
    };
    let extract = |a: &zbus::zvariant::Array| -> Vec<String> {
        a.inner().iter().filter_map(|v| {
            if let Value::Str(s) = v { Some(s.to_string()) } else { None }
        }).collect()
    };
    match &*outer {
        Value::Value(inner) => match inner.as_ref() { Value::Array(a) => extract(a), _ => Vec::new() },
        Value::Array(a)     => extract(a),
        _                   => Vec::new(),
    }
}

fn err_is_unknown_object(e: &zbus::Error) -> bool {
    let s = e.to_string();
    s.contains("UnknownObject")
        || s.contains("Unknown object")
        || s.contains("No such object path")
        || s.contains("does not exist at path")
        || (s.contains("No such interface") && s.contains("DBus.Properties"))
        || (s.contains("no such interface") && s.contains("DBus.Properties"))
}

fn xml_has_sni_interface(xml: &str) -> bool {
    xml.contains("<interface name=\"org.kde.StatusNotifierItem\"")
        || xml.contains("<interface name=\"org.ayatana.AppIndicator\"")
        || xml.contains("<interface name=\"org.freedesktop.StatusNotifierItem\"")
}

fn xml_has_properties_interface(xml: &str) -> bool {
    xml.contains("<interface name=\"org.freedesktop.DBus.Properties\"")
}

fn xml_child_names(xml: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in xml.lines() {
        let line = line.trim();
        if !line.starts_with("<node name=") { continue; }
        if let Some(s) = line.find('"') {
            let rest = &line[s + 1..];
            if let Some(e) = rest.find('"') {
                let name = &rest[..e];
                if !name.is_empty() && name != "/" { names.push(name.to_string()); }
            }
        }
    }
    names
}

/// Returns Ok(Some(xml)) on success, Ok(None) when Introspectable is absent
/// (caller should proceed), Err(()) on timeout / unknown object (caller should abort).
async fn try_introspect(conn: &Connection, bus: &str, path: &str) -> Result<Option<String>, ()> {
    match tokio::time::timeout(T_PROBE, conn.call_method(
        Some(bus), path, Some("org.freedesktop.DBus.Introspectable"), "Introspect", &(),
    )).await {
        Err(_)                                   => Err(()),
        Ok(Err(e)) if err_is_unknown_object(&e) => Err(()),
        Ok(Err(_))                               => Ok(None),
        Ok(Ok(msg)) => Ok(msg.body().deserialize::<String>().ok().filter(|s| !s.is_empty())),
    }
}

/// Introspect and return XML, or None on any error/absence.
async fn introspect_xml(conn: &Connection, bus: &str, path: &str) -> Option<String> {
    try_introspect(conn, bus, path).await.ok().flatten()
}

async fn scan_one_bus_name(conn: &Connection, bus_name: &str, items: TrayItems) {
    let mut found_any = false;

    // Pass 1: Ayatana / libappindicator.
    if let Some(xml) = introspect_xml(conn, bus_name, "/org/ayatana/NotificationItem").await {
        if xml_has_sni_interface(&xml) {
            let svc = format!("{bus_name}/org/ayatana/NotificationItem");
            if fetch_and_watch(conn, &svc, Arc::clone(&items)).await { found_any = true; }
        }
        for child in xml_child_names(&xml) {
            let svc = format!("{bus_name}/org/ayatana/NotificationItem/{child}");
            if fetch_and_watch(conn, &svc, Arc::clone(&items)).await { found_any = true; }
        }
    }

    // Pass 2: well-known SNI paths.
    for path in SNI_PATHS {
        let should_probe = match try_introspect(conn, bus_name, path).await {
            Err(())       => false,
            Ok(None)      => true,
            Ok(Some(xml)) => xml.is_empty() || xml_has_sni_interface(&xml) || xml_has_properties_interface(&xml),
        };
        if should_probe {
            let svc = format!("{bus_name}{path}");
            if fetch_and_watch(conn, &svc, Arc::clone(&items)).await { found_any = true; }
        }
    }

    // Pass 3: deep introspect fallback.
    if !found_any {
        if let Some(found_path) = introspect_find_sni_path(conn, bus_name).await {
            let svc = format!("{bus_name}{found_path}");
            fetch_and_watch(conn, &svc, Arc::clone(&items)).await;
        }
    }
}

async fn introspect_find_sni_path(conn: &Connection, bus_name: &str) -> Option<String> {
    const ROOTS: &[&str] = &[
        "/StatusNotifierItem", "/org/ayatana/NotificationItem",
        "/org/kde/StatusNotifierItem", "/org", "/",
    ];
    for root in ROOTS {
        let xml = introspect_xml(conn, bus_name, root).await?;
        if xml_has_sni_interface(&xml) { return Some(root.to_string()); }

        for child in xml_child_names(&xml) {
            let child_path = if *root == "/" { format!("/{child}") } else { format!("{root}/{child}") };
            if let Some(cxml) = introspect_xml(conn, bus_name, &child_path).await {
                if xml_has_sni_interface(&cxml) { return Some(child_path); }
                for grandchild in xml_child_names(&cxml) {
                    let gpath = format!("{child_path}/{grandchild}");
                    if let Some(gxml) = introspect_xml(conn, bus_name, &gpath).await {
                        if xml_has_sni_interface(&gxml) { return Some(gpath); }
                    }
                }
            }
        }
    }
    None
}

async fn probe_service(conn: &Connection, service: &str, items: TrayItems) {
    let (bus_name, obj_path) = split_service(service);
    let unique = if bus_name.starts_with(':') {
        bus_name.to_string()
    } else {
        match resolve_unique_name(conn, bus_name).await { Some(u) => u, None => return }
    };
    let canonical = format!("{unique}{obj_path}");
    let ok = tokio::time::timeout(T_FETCH, fetch_and_watch(conn, &canonical, Arc::clone(&items)))
        .await.unwrap_or(false);
    if !ok {
        if let Some(p) = introspect_find_sni_path(conn, &unique).await {
            let found = format!("{unique}{p}");
            let _ = tokio::time::timeout(T_FETCH, fetch_and_watch(conn, &found, Arc::clone(&items))).await;
        }
    }
}

// ============================================================================
// Icon fetching + per-item signal watcher
// ============================================================================

async fn fetch_and_watch(conn: &Connection, service: &str, items: TrayItems) -> bool {
    if fetch_icon(conn, service, Arc::clone(&items)).await {
        let (conn2, items2, svc) = (conn.clone(), Arc::clone(&items), service.to_string());
        tokio::spawn(async move { watch_sni_signals(&conn2, &svc, items2).await; });
        true
    } else {
        false
    }
}

async fn watch_sni_signals(conn: &Connection, service: &str, items: TrayItems) {
    let (bus_name, _) = split_service(service);
    let service_owned = service.to_string();

    // Build a match-rule stream; returns None on error.
    let make_stream = |iface: &'static str, member: Option<&'static str>| {
        let conn = conn;
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(bus_name).ok()
            .and_then(|b| b.interface(iface).ok())
            .and_then(|b| match member { None => Some(b), Some(m) => b.member(m).ok() })
            .map(|b| b.build());
        async move {
            let r = rule?;
            zbus::MessageStream::for_match_rule(r, conn, None).await.ok()
        }
    };

    let Some(sni_stream)  = make_stream("org.kde.StatusNotifierItem", None).await else { return };
    let aya_stream        = make_stream("org.ayatana.AppIndicator", None).await;
    let Some(prop_stream) = make_stream("org.freedesktop.DBus.Properties", Some("PropertiesChanged")).await else { return };
    let menu_stream       = make_stream("com.canonical.dbusmenu", Some("LayoutUpdated")).await;

    type MsgResult = Result<zbus::Message, zbus::Error>;
    type Tagged<'a> = futures_util::stream::BoxStream<'a, (u8, MsgResult)>;

    let s1: Tagged<'_> = match aya_stream {
        Some(s) => s.map(|r| (1u8, r)).boxed(),
        None    => futures_util::stream::empty().boxed(),
    };
    let s3: Tagged<'_> = match menu_stream {
        Some(s) => s.map(|r| (3u8, r)).boxed(),
        None    => futures_util::stream::empty().boxed(),
    };
    let mut merged = futures_util::stream::select(
        futures_util::stream::select(sni_stream.map(|r| (0u8, r)).boxed(), s1),
        futures_util::stream::select(prop_stream.map(|r| (2u8, r)).boxed(), s3),
    );

    while let Some((source, result)) = merged.next().await {
        let member: Option<String> = match result {
            Err(_) => None,
            Ok(_) if source == 2 => Some("PropertiesChanged".into()),
            Ok(_) if source == 3 => Some("LayoutUpdated".into()),
            Ok(m)                => m.header().member().map(|n: &zbus::names::MemberName| n.as_str().to_string()),
        };

        let is_icon = matches!(member.as_deref(),
            Some("NewIcon") | Some("NewOverlayIcon") | Some("NewAttentionIcon") |
            Some("NewIconThemePath") | Some("NewStatus") | Some("NewToolTip") |
            Some("NewTitle") | Some("NewLabel") | Some("PropertiesChanged")
        );
        let is_menu = member.as_deref() == Some("LayoutUpdated");

        if is_icon { fetch_icon(conn, &service_owned, Arc::clone(&items)).await; }

        if is_menu {
            let menu_info = {
                let locked = items.lock().unwrap();
                locked.iter().find(|i| i.id == service_owned)
                    .and_then(|i| i.menu_loaded.then(|| {
                        i.menu_path.as_ref().map(|p| (i.bus_name.clone(), p.clone()))
                    })).flatten()
            };
            if let Some((bus, path)) = menu_info {
                fetch_menu_internal(conn, &bus, &path, &service_owned, Arc::clone(&items)).await;
            }
        }
    }
}

// ============================================================================
// Core icon fetching
// ============================================================================

type PropMap = HashMap<String, zbus::zvariant::OwnedValue>;

async fn resolve_unique_name(conn: &Connection, name: &str) -> Option<String> {
    let dbus = zbus::fdo::DBusProxy::new(conn).await.ok()?;
    dbus.get_name_owner(name.try_into().ok()?).await.ok().map(|n| n.to_string())
}

/// Try GetAll with each known interface, then unfiltered, then no-args.
/// Returns the first PropMap containing the mandatory "Id" key.
async fn fetch_props(conn: &Connection, bus: &str, path: &str) -> PropMap {
    for iface in SNI_INTERFACES {
        if let Ok(m) = conn.call_method(
            Some(bus), path, Some("org.freedesktop.DBus.Properties"), "GetAll", &(iface,),
        ).await {
            let map: PropMap = m.body().deserialize().unwrap_or_default();
            if map.contains_key("Id") { return map; }
        }
    }
    if let Ok(m) = conn.call_method(
        Some(bus), path, Some("org.freedesktop.DBus.Properties"), "GetAll", &("",),
    ).await {
        let map: PropMap = m.body().deserialize().unwrap_or_default();
        if map.contains_key("Id") { return map; }
    }
    if let Ok(m) = conn.call_method(
        Some(bus), path, Some("org.freedesktop.DBus.Properties"), "GetAll", &(),
    ).await {
        let map: PropMap = m.body().deserialize().unwrap_or_default();
        if map.contains_key("Id") { return map; }
    }
    PropMap::new()
}

/// Last-resort: fetch each property individually.
async fn fetch_props_individually(conn: &Connection, bus: &str, path: &str) -> PropMap {
    use zbus::zvariant::Value;
    const PROPS: &[&str] = &[
        "Id", "Category", "Status", "Title",
        "IconName", "IconThemePath", "IconPixmap",
        "AttentionIconName", "AttentionIconThemePath", "AttentionIconPixmap",
        "OverlayIconName", "OverlayIconPixmap", "ToolTip", "ItemIsMenu", "Menu",
    ];

    for iface in SNI_INTERFACES {
        let id_msg = match conn.call_method(
            Some(bus), path, Some("org.freedesktop.DBus.Properties"), "Get", &(iface, "Id"),
        ).await { Ok(m) => m, Err(_) => continue };

        let id_val: zbus::zvariant::OwnedValue = match id_msg.body().deserialize() {
            Ok(v) => v, Err(_) => continue,
        };
        let id_inner = match &*id_val {
            Value::Value(v) => zbus::zvariant::OwnedValue::try_from(v.as_ref()).ok(),
            _               => id_val.try_clone().ok(),
        };
        let id_str: Option<String> = id_inner.as_ref().and_then(|v| {
            if let Value::Str(s) = &**v { Some(s.to_string()) } else { None }
        });
        if id_str.as_deref().map_or(true, str::is_empty) { continue; }

        let mut map = PropMap::new();
        if let Some(val) = id_inner { map.insert("Id".into(), val); }

        for prop in PROPS.iter().filter(|&&p| p != "Id") {
            let Ok(msg) = conn.call_method(
                Some(bus), path, Some("org.freedesktop.DBus.Properties"), "Get", &(iface, prop),
            ).await else { continue };
            let Ok(val): Result<zbus::zvariant::OwnedValue, _> = msg.body().deserialize() else { continue };
            let inner = match &*val {
                Value::Value(v) => zbus::zvariant::OwnedValue::try_from(v.as_ref()).ok(),
                _               => Some(val),
            };
            if let Some(inner) = inner { map.insert(prop.to_string(), inner); }
        }
        return map;
    }
    PropMap::new()
}

async fn fetch_icon(conn: &Connection, service: &str, items: TrayItems) -> bool {
    let (bus_name, obj_path) = split_service(service);
    let effective_bus = if bus_name.starts_with(':') {
        bus_name.to_string()
    } else {
        resolve_unique_name(conn, bus_name).await.unwrap_or_else(|| bus_name.to_string())
    };
    let bus = effective_bus.as_str();

    match try_introspect(conn, bus, obj_path).await {
        Err(())       => return false,
        Ok(Some(xml)) if !xml.is_empty() && !xml_has_sni_interface(&xml) && !xml_has_properties_interface(&xml) => return false,
        _             => {}
    }

    let mut all = fetch_props(conn, bus, obj_path).await;
    if all.is_empty() { all = fetch_props_individually(conn, bus, obj_path).await; }
    if all.is_empty() { return false; }

    let id_str = match prop_str(&all, "Id").filter(|s| !s.is_empty()) {
        Some(s) => s, None => return false,
    };

    let category = match prop_str(&all, "Category").as_deref() {
        Some("Communications") => TrayCategory::Communications,
        Some("SystemServices") => TrayCategory::SystemServices,
        Some("Hardware")       => TrayCategory::Hardware,
        _                      => TrayCategory::ApplicationStatus,
    };
    let status = match prop_str(&all, "Status").as_deref() {
        Some("Passive")        => TrayStatus::Passive,
        Some("NeedsAttention") => TrayStatus::NeedsAttention,
        _                      => TrayStatus::Active,
    };
    let (tooltip_title, tooltip_body) = parse_tooltip(&all).unwrap_or_else(|| {
        let title = prop_str(&all, "Title").filter(|s| !s.is_empty()).unwrap_or(id_str.clone());
        (title, String::new())
    });

    let (icon_w, icon_h, icon_rgba)                               = unpack_pixmap(all.get("IconPixmap"));
    let (attention_icon_w, attention_icon_h, attention_icon_rgba) = unpack_pixmap(all.get("AttentionIconPixmap"));
    let (overlay_icon_w, overlay_icon_h, overlay_icon_rgba)       = unpack_pixmap(all.get("OverlayIconPixmap"));

    let new_icon = TrayIcon {
        id:       service.to_string(),
        bus_name: bus.to_string(),
        obj_path: obj_path.to_string(),
        category,
        icon_rgba, icon_w, icon_h,
        icon_name:            prop_str(&all, "IconName").filter(|s| !s.is_empty()),
        icon_theme_path:      prop_str(&all, "IconThemePath").filter(|s| !s.is_empty()),
        attention_icon_rgba, attention_icon_w, attention_icon_h,
        attention_icon_name:  prop_str(&all, "AttentionIconName").filter(|s| !s.is_empty()),
        overlay_icon_rgba, overlay_icon_w, overlay_icon_h,
        overlay_icon_name:    prop_str(&all, "OverlayIconName").filter(|s| !s.is_empty()),
        status,
        item_is_menu: prop_bool(&all, "ItemIsMenu"),
        tooltip_title,
        tooltip_body,
        menu_path:     prop_obj_path(&all, "Menu"),
        menu_items:    Vec::new(),
        menu_revision: 0,
        menu_loaded:   false,
        icon_rev:      0,
    };

    let mut locked = items.lock().unwrap();
    if let Some(existing) = locked.iter_mut().find(|i| i.id == new_icon.id) {
        let changed = existing.icon_rgba != new_icon.icon_rgba
            || existing.attention_icon_rgba != new_icon.attention_icon_rgba;
        let new_rev = if changed { existing.icon_rev.wrapping_add(1) } else { existing.icon_rev };
        let (menu_items, menu_revision, menu_loaded) =
            (existing.menu_items.clone(), existing.menu_revision, existing.menu_loaded);
        *existing = new_icon;
        existing.icon_rev      = new_rev;
        existing.menu_items    = menu_items;
        existing.menu_revision = menu_revision;
        existing.menu_loaded   = menu_loaded;
    } else {
        locked.push(new_icon);
    }
    true
}

// ============================================================================
// DBusMenu fetching
// ============================================================================

async fn fetch_menu_internal(
    conn: &Connection, bus_name: &str, menu_path: &str, service_id: &str, items: TrayItems,
) {
    let result = conn.call_method(
        Some(bus_name), menu_path,
        Some("com.canonical.dbusmenu"), "GetLayout",
        &(0i32, -1i32, Vec::<String>::new()),
    ).await;
    let msg = match result { Ok(m) => m, Err(_) => { mark_menu_loaded(&items, service_id); return; } };

    type MenuNodeRaw = (i32, HashMap<String, zbus::zvariant::OwnedValue>, Vec<zbus::zvariant::OwnedValue>);
    let (revision, root_node): (u32, MenuNodeRaw) = match msg.body().deserialize() {
        Ok(v) => v, Err(_) => { mark_menu_loaded(&items, service_id); return; }
    };

    let menu_items = parse_menu_items(&root_node.2);
    let mut locked = items.lock().unwrap();
    if let Some(icon) = locked.iter_mut().find(|i| i.id == service_id) {
        icon.menu_items    = menu_items;
        icon.menu_revision = revision;
        icon.menu_loaded   = true;
    }
}

fn mark_menu_loaded(items: &TrayItems, service_id: &str) {
    if let Some(icon) = items.lock().unwrap().iter_mut().find(|i| i.id == service_id) {
        icon.menu_loaded = true;
    }
}

fn parse_menu_items(children: &[zbus::zvariant::OwnedValue]) -> Vec<MenuItem> {
    use zbus::zvariant::Value;
    let mut items = Vec::new();

    for child_val in children {
        let inner: &Value = match &**child_val { Value::Value(b) => b.as_ref(), other => other };
        let fields = match inner { Value::Structure(s) => s.fields(), _ => continue };
        if fields.len() < 3 { continue; }
        let id = match &fields[0] { Value::I32(v) => *v, _ => continue };

        let props: HashMap<String, zbus::zvariant::OwnedValue> = match &fields[1] {
            Value::Dict(d) => d.iter().filter_map(|(k, v)| {
                let key = match k { Value::Str(s) => s.to_string(), _ => return None };
                Some((key, zbus::zvariant::OwnedValue::try_from(v).ok()?))
            }).collect(),
            _ => HashMap::new(),
        };

        let prop = |k: &str| -> Option<String> { props.get(k).and_then(|v| string_from_value(&**v)) };

        let visible = prop("visible").map(|v| v != "false").unwrap_or(true);
        if !visible { continue; }

        let children_nested: Vec<zbus::zvariant::OwnedValue> = match &fields[2] {
            Value::Array(a) => a.inner().iter()
                .filter_map(|v| zbus::zvariant::OwnedValue::try_from(v).ok()).collect(),
            _ => Vec::new(),
        };

        items.push(MenuItem {
            id,
            label:        prop("label").unwrap_or_default().replace('_', ""),
            enabled:      prop("enabled").map(|v| v != "false").unwrap_or(true),
            visible,
            is_separator: prop("type").map(|t| t == "separator").unwrap_or(false),
            icon_name:    prop("icon-name").filter(|s| !s.is_empty()),
            toggle_type:  match prop("toggle-type").as_deref() {
                Some("checkmark") => ToggleType::Checkmark,
                Some("radio")     => ToggleType::Radio,
                _                 => ToggleType::None,
            },
            toggle_state: prop("toggle-state").and_then(|s| s.parse().ok()).unwrap_or(-1),
            children:     parse_menu_items(&children_nested),
        });
    }
    items
}

fn string_from_value(v: &zbus::zvariant::Value) -> Option<String> {
    use zbus::zvariant::Value;
    match v {
        Value::Value(inner) => string_from_value(inner),
        Value::Str(s)       => Some(s.to_string()),
        Value::Bool(b)      => Some(b.to_string()),
        Value::I32(n)       => Some(n.to_string()),
        Value::U32(n)       => Some(n.to_string()),
        _                   => None,
    }
}

// ============================================================================
// PropMap helpers
// ============================================================================

fn prop_str(map: &PropMap, key: &str) -> Option<String> {
    use zbus::zvariant::Value;
    match &**map.get(key)? {
        Value::Str(s)   => Some(s.to_string()),
        Value::Value(v) => if let Value::Str(s) = v.as_ref() { Some(s.to_string()) } else { None },
        _               => None,
    }
}

fn prop_obj_path(map: &PropMap, key: &str) -> Option<String> {
    use zbus::zvariant::Value;
    match &**map.get(key)? {
        Value::ObjectPath(p) => {
            let s = p.as_str();
            if s == "/" || s.is_empty() { None } else { Some(s.to_string()) }
        }
        Value::Str(s) if !s.is_empty() && s != "/" => Some(s.to_string()),
        _ => None,
    }
}

fn prop_bool(map: &PropMap, key: &str) -> bool {
    use zbus::zvariant::Value;
    match map.get(key).map(|v| &**v) {
        Some(Value::Bool(b))  => *b,
        Some(Value::Value(v)) => matches!(v.as_ref(), Value::Bool(true)),
        _                     => false,
    }
}

fn parse_tooltip(map: &PropMap) -> Option<(String, String)> {
    use zbus::zvariant::Value;
    let raw = map.get("ToolTip")?;
    // Peel off variant wrapper if present, keeping a reference into raw.
    let deref: &Value = match &**raw { Value::Value(v) => v.as_ref(), other => other };
    let fields = match deref { Value::Structure(s) => s.fields(), _ => return None };
    if fields.len() < 4 { return None; }
    let title = match &fields[2] { Value::Str(s) => s.to_string(), _ => String::new() };
    let body  = match &fields[3] { Value::Str(s) => s.to_string(), _ => String::new() };
    if title.is_empty() && body.is_empty() { return None; }
    Some((title, body))
}

fn parse_icon_pixmap(val: &zbus::zvariant::OwnedValue) -> Option<(u32, u32, Vec<u8>)> {
    use zbus::zvariant::Value;
    let arr = match &**val { Value::Array(a) => a, _ => return None };
    let mut best: Option<(u32, u32, Vec<u8>)> = None;

    for item in arr.inner() {
        let fields = match item { Value::Structure(s) => s.fields(), _ => continue };
        if fields.len() < 3 { continue; }
        let w = match &fields[0] { Value::I32(v) => *v as u32, _ => continue };
        let h = match &fields[1] { Value::I32(v) => *v as u32, _ => continue };
        let raw: Vec<u8> = match &fields[2] {
            Value::Array(a) => a.inner().iter()
                .filter_map(|b| if let Value::U8(b) = b { Some(*b) } else { None }).collect(),
            _ => continue,
        };
        if raw.is_empty() || w == 0 || h == 0 { continue; }
        let area = w * h;
        if best.as_ref().map_or(true, |&(bw, bh, _)| area > bw * bh) {
            best = Some((w, h, argb_to_rgba(&raw)));
        }
    }
    best
}

fn unpack_pixmap(val: Option<&zbus::zvariant::OwnedValue>) -> (u32, u32, Vec<u8>) {
    val.and_then(parse_icon_pixmap).unwrap_or((0, 0, Vec::new()))
}

/// Convert ARGB32 (big-endian, as spec'd by SNI) to RGBA (as expected by egui).
/// Called once per pixmap in `parse_icon_pixmap`; do NOT call again in the GUI.
pub fn argb_to_rgba(argb: &[u8]) -> Vec<u8> {
    argb.chunks_exact(4).flat_map(|c| [c[1], c[2], c[3], c[0]]).collect()
}

// ============================================================================
// Misc
// ============================================================================

fn split_service(service: &str) -> (&str, &str) {
    match service.find('/') {
        Some(pos) => (&service[..pos], &service[pos..]),
        None      => (service, "/StatusNotifierItem"),
    }
}
