//! SNI (StatusNotifierItem) host + watcher + DBusMenu implementation.
//!
//! Design:
//! - Tries to claim `org.kde.StatusNotifierWatcher` (and the freedesktop alias)
//!   so new apps register directly with us.
//! - Queries existing watchers for already-registered items on startup.
//! - Subscribes to `StatusNotifierItemRegistered` signals from all watchers
//!   so we catch new apps even when another watcher (KDE, waybar) is active.
//! - Per-item signal tasks subscribe to `NewIcon`, `NewStatus`, `NewToolTip`
//!   etc. so icons refresh without polling.
//! - Items are removed when their bus name vanishes.
use std::sync::{Arc, Mutex};
use std::thread;
use std::collections::HashMap;

use zbus::{interface, Connection, ConnectionBuilder};
use crate::gui::Config;

// ============================================================================
// Public types
// ============================================================================

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct MenuItem {
    pub id:           i32,
    pub label:        String,
    pub enabled:      bool,
    pub visible:      bool,
    pub is_separator: bool,
    pub children:     Vec<MenuItem>,
}

/// Status as reported by `org.kde.StatusNotifierItem.Status`.
/// `Passive`=hidden/idle, `Active`=normal, `NeedsAttention`=show attention icon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TrayStatus { #[default] Active, Passive, NeedsAttention }

#[derive(Clone, Debug, Default)]
pub struct TrayIcon {
    pub id:              String,
    pub bus_name:        String,
    pub obj_path:        String,

    /// Normal icon (pixmap path, if app supplied raw ARGB32 data).
    pub icon_rgba:       Vec<u8>,
    pub icon_w:          u32,
    pub icon_h:          u32,
    /// Freedesktop icon name for the normal icon (preferred over pixmap).
    pub icon_name:       Option<String>,
    pub icon_theme_path: Option<String>,

    /// Icon shown when status == NeedsAttention.
    pub attention_icon_rgba:  Vec<u8>,
    pub attention_icon_w:     u32,
    pub attention_icon_h:     u32,
    pub attention_icon_name:  Option<String>,

    /// Overlay icon (drawn on top of the main icon).
    pub overlay_icon_rgba: Vec<u8>,
    pub overlay_icon_w:    u32,
    pub overlay_icon_h:    u32,
    pub overlay_icon_name: Option<String>,

    /// Item status — drives which icon to display and whether to hide the item.
    pub status: TrayStatus,

    /// When true the item only supports a context menu; left-click should call
    /// `ContextMenu()` instead of `Activate()`.
    pub item_is_menu: bool,

    /// Tooltip text (spec property `ToolTip`, fields 2 and 3 of the struct).
    pub tooltip_title: String,
    pub tooltip_body:  String,

    pub menu_path:     Option<String>,

    /// Menu layout fetched from DBusMenu; populated on first right-click.
    pub menu_items:    Vec<MenuItem>,
    pub menu_revision: u32,
    /// Set to true once a GetLayout call has completed (even if it returned 0
    /// items), so the GUI can distinguish "still loading" from "loaded but empty".
    pub menu_loaded:   bool,
}

pub type TrayItems = Arc<Mutex<Vec<TrayIcon>>>;

#[allow(dead_code)]
pub enum SniAction {
    Activate          { bus_name: String, obj_path: String },
    SecondaryActivate { bus_name: String, obj_path: String },
    /// Tell the item to show its own native context menu at (x, y).
    /// Used when `ItemIsMenu == true` or on explicit right-click without dbusmenu.
    ContextMenu       { bus_name: String, obj_path: String, x: i32, y: i32 },
    /// Mouse wheel event forwarded to the item (e.g. volume knobs).
    Scroll            { bus_name: String, obj_path: String, delta: i32, orientation: String },
    MenuAboutToShow   { bus_name: String, menu_path: String },
    MenuEvent         { bus_name: String, menu_path: String, item_id: i32 },
    FetchMenu         { bus_name: String, menu_path: String, service_id: String },
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
        let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel::<SniAction>();

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

    pub fn activate(&self, bus_name: &str, obj_path: &str) {
        let _ = self.action_tx.send(SniAction::Activate {
            bus_name: bus_name.to_string(),
            obj_path: obj_path.to_string(),
        });
    }

    #[allow(dead_code)]
    pub fn secondary_activate(&self, bus_name: &str, obj_path: &str) {
        let _ = self.action_tx.send(SniAction::SecondaryActivate {
            bus_name: bus_name.to_string(),
            obj_path: obj_path.to_string(),
        });
    }

    /// Request menu layout fetch; result is written back to TrayItems.
    pub fn fetch_menu(&self, bus_name: &str, menu_path: &str, service_id: &str) {
        let _ = self.action_tx.send(SniAction::FetchMenu {
            bus_name:   bus_name.to_string(),
            menu_path:  menu_path.to_string(),
            service_id: service_id.to_string(),
        });
    }

    /// Forward a scroll-wheel event to the item (spec `Scroll(delta, orientation)`).
    /// `orientation` is `"vertical"` or `"horizontal"`.
    pub fn scroll(&self, bus_name: &str, obj_path: &str, delta: i32, orientation: &str) {
        let _ = self.action_tx.send(SniAction::Scroll {
            bus_name:    bus_name.to_string(),
            obj_path:    obj_path.to_string(),
            delta,
            orientation: orientation.to_string(),
        });
    }

    /// Ask the item to show its own native context menu at screen position (x, y).
    /// Use this when `TrayIcon::item_is_menu == true` (left-click) or for
    /// right-click when the item has no dbusmenu path.
    pub fn context_menu(&self, bus_name: &str, obj_path: &str, x: i32, y: i32) {
        let _ = self.action_tx.send(SniAction::ContextMenu {
            bus_name: bus_name.to_string(),
            obj_path: obj_path.to_string(),
            x, y,
        });
    }

    /// Notify the menu it is about to be shown (required by spec).
    pub fn menu_about_to_show(&self, bus_name: &str, menu_path: &str) {
        let _ = self.action_tx.send(SniAction::MenuAboutToShow {
            bus_name:  bus_name.to_string(),
            menu_path: menu_path.to_string(),
        });
    }

    /// Fire a menu item click event.
    pub fn menu_event(&self, bus_name: &str, menu_path: &str, item_id: i32) {
        let _ = self.action_tx.send(SniAction::MenuEvent {
            bus_name:  bus_name.to_string(),
            menu_path: menu_path.to_string(),
            item_id,
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
        let sender = hdr
            .sender()
            .map(|s: &zbus::names::UniqueName| s.to_string())
            .unwrap_or_default();

        // Build the canonical "bus_name/obj_path" key.
        let full = if service.starts_with('/') {
            // Service is an object path; bus name is the sender's unique name.
            format!("{sender}{service}")
        } else if service.is_empty() {
            format!("{sender}/StatusNotifierItem")
        } else if service.contains('/') {
            // Already "busname/path" or "unique/path".
            service
        } else {
            // Well-known name without a path.
            format!("{service}/StatusNotifierItem")
        };

        {
            let mut reg = self.registered.lock().unwrap();
            if reg.contains(&full) { return; }
            reg.push(full.clone());
        }

        eprintln!("SNI: registered {full}");

        // Emit StatusNotifierItemRegistered so other hosts / apps know.
        // The signal argument is the service string as registered (spec §Watcher).
        {
            let conn2  = conn.clone();
            let full2  = full.clone();
            tokio::spawn(async move {
                if let Ok(ctx) = zbus::SignalContext::new(&conn2, "/StatusNotifierWatcher") {
                    let _ = Watcher::status_notifier_item_registered(&ctx, &full2).await;
                }
            });
        }

        let items = Arc::clone(&self.items);
        let conn  = conn.clone();
        tokio::spawn(async move {
            fetch_and_watch(&conn, &full, items).await;
        });
    }

    async fn register_status_notifier_host(&self, _service: String) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registered.lock().unwrap().clone()
    }
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool { true }
    #[zbus(property)]
    fn protocol_version(&self) -> i32 { 0 }

    #[zbus(signal)]
    async fn status_notifier_item_registered(ctxt: &zbus::SignalContext<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(ctxt: &zbus::SignalContext<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(ctxt: &zbus::SignalContext<'_>) -> zbus::Result<()>;
}

// ============================================================================
// Watcher startup
// ============================================================================

async fn run_watcher(
    items:         TrayItems,
    mut action_rx: tokio::sync::mpsc::UnboundedReceiver<SniAction>,
) -> zbus::Result<()> {
    eprintln!("SNI: starting");

    // Plain connection used for everything except serving the watcher interface.
    let conn = Connection::session().await?;

    // ------------------------------------------------------------------
    // Try to become the StatusNotifierWatcher.
    // If another process (KDE, waybar) already holds the name we skip this
    // and just consume from whichever watcher is active.
    // ------------------------------------------------------------------
    let watcher_conn = try_become_watcher(Arc::clone(&items)).await;
    eprintln!("SNI: watcher role: {}", if watcher_conn.is_some() { "claimed" } else { "not claimed" });

    // Register as a host so apps know a consumer is present.
    let host_name = format!("org.kde.StatusNotifierHost-{}", std::process::id());
    let _ = conn.request_name(host_name.as_str()).await;

    // Emit StatusNotifierHostRegistered so already-running apps re-register.
    if let Some(ref wc) = watcher_conn {
        if let Ok(ctx) = zbus::SignalContext::new(wc, "/StatusNotifierWatcher") {
            let _ = Watcher::status_notifier_host_registered(&ctx).await;
            eprintln!("SNI: emitted StatusNotifierHostRegistered");
        }
    }

    // ------------------------------------------------------------------
    // Harvest items already registered with any active watcher.
    // This is the primary way we pick up apps that started before us.
    // ------------------------------------------------------------------
    let watcher_names = ["org.kde.StatusNotifierWatcher", "org.freedesktop.StatusNotifierWatcher"];
    for wname in &watcher_names {
        let registered = query_watcher_items(&conn, wname).await;
        eprintln!("SNI: {wname} has {} registered item(s): {:?}", registered.len(), registered);
        for service in registered {
            let c = conn.clone();
            let i = Arc::clone(&items);
            tokio::spawn(async move {
                eprintln!("SNI: probing watcher item: {service}");
                probe_service(&c, &service, i).await;
            });
        }
    }

    // ------------------------------------------------------------------
    // Scan ALL bus names for SNI items.
    //
    // Many apps (Discord, PulseAudio indicator, Electron apps) do not
    // re-register when a new host appears — they only register once on
    // startup.  We enumerate every unique name on the session bus and
    // probe each for a StatusNotifierItem object.
    // ------------------------------------------------------------------
    {
        // Use a raw call to avoid lifetime issues with the typed DBusProxy API.
        let names_msg = conn.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "ListNames",
            &(),
        ).await;

        match names_msg {
            Err(e) => eprintln!("SNI: ListNames failed: {e}"),
            Ok(msg) => {
                let all_names: Vec<String> = msg.body().deserialize().unwrap_or_default();
                let unique_names: Vec<String> = all_names
                    .into_iter()
                    .filter(|n| n.starts_with(':'))
                    .collect();

                eprintln!("SNI: scanning {} bus names for SNI items", unique_names.len());

                for name in unique_names {
                    let c = conn.clone();
                    let i = Arc::clone(&items);
                    tokio::spawn(async move {
                        scan_one_bus_name(&c, &name, i).await;
                    });
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Watch StatusNotifierItemRegistered signals from all active watchers.
    // This covers apps that register after we start, in the case where
    // another watcher holds the name.
    // ------------------------------------------------------------------
    for wname in &watcher_names {
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
            let c = conn.clone();
            let i = Arc::clone(&items);
            tokio::spawn(async move {
                while let Some(Ok(msg)) = futures_util::StreamExt::next(&mut stream).await {
                    if let Ok(service) = msg.body().deserialize::<String>() {
                        eprintln!("SNI: watcher signalled new item: {service}");
                        let c2 = c.clone();
                        let i2 = Arc::clone(&i);
                        tokio::spawn(async move {
                            probe_service(&c2, &service, i2).await;
                        });
                    }
                }
            });
        }
    }

    // ------------------------------------------------------------------
    // Watch for bus names appearing and vanishing.
    //   - New unique names  → probe for SNI items (catches apps that start
    //     after us without calling RegisterStatusNotifierItem on our watcher).
    //   - Names going away  → remove their tray items.
    // ------------------------------------------------------------------
    {
        let dbus       = zbus::fdo::DBusProxy::new(&conn).await?;
        let mut stream = dbus.receive_name_owner_changed().await?;
        let items_w    = Arc::clone(&items);
        let conn_w     = conn.clone();
        tokio::spawn(async move {
            while let Some(sig) = futures_util::StreamExt::next(&mut stream).await {
                let Ok(args) = sig.args() else { continue };
                let name = args.name().to_string();

                if args.new_owner().is_some() {
                    // A new bus name appeared. Only probe unique names (:1.xxx)
                    // so we don't double-probe well-known aliases.
                    if name.starts_with(':') {
                        let c = conn_w.clone();
                        let i = Arc::clone(&items_w);
                        tokio::spawn(async move {
                            scan_one_bus_name(&c, &name, i).await;
                        });
                    }
                } else {
                    // Name vanished — remove its tray items.
                    let mut locked = items_w.lock().unwrap();
                    let before = locked.len();
                    locked.retain(|i| {
                        i.bus_name != name && !i.id.starts_with(&format!("{name}/"))
                    });
                    if locked.len() < before {
                        eprintln!("SNI: removed {} item(s) for vanished {name}", before - locked.len());
                    }
                }
            }
        });
    }

    // ------------------------------------------------------------------
    // Action handler.
    // ------------------------------------------------------------------
    let conn_act  = conn.clone();
    let items_act = Arc::clone(&items);
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            match action {
                SniAction::Activate { bus_name, obj_path } => {
                    let _ = conn_act.call_method(
                        Some(bus_name.as_str()), obj_path.as_str(),
                        Some("org.kde.StatusNotifierItem"), "Activate",
                        &(0i32, 0i32),
                    ).await;
                }
                SniAction::SecondaryActivate { bus_name, obj_path } => {
                    let _ = conn_act.call_method(
                        Some(bus_name.as_str()), obj_path.as_str(),
                        Some("org.kde.StatusNotifierItem"), "SecondaryActivate",
                        &(0i32, 0i32),
                    ).await;
                }
                SniAction::ContextMenu { bus_name, obj_path, x, y } => {
                    let _ = conn_act.call_method(
                        Some(bus_name.as_str()), obj_path.as_str(),
                        Some("org.kde.StatusNotifierItem"), "ContextMenu",
                        &(x, y),
                    ).await;
                }
                SniAction::Scroll { bus_name, obj_path, delta, orientation } => {
                    let _ = conn_act.call_method(
                        Some(bus_name.as_str()), obj_path.as_str(),
                        Some("org.kde.StatusNotifierItem"), "Scroll",
                        &(delta, orientation.as_str()),
                    ).await;
                }
                SniAction::MenuAboutToShow { bus_name, menu_path } => {
                    let _ = conn_act.call_method(
                        Some(bus_name.as_str()), menu_path.as_str(),
                        Some("com.canonical.dbusmenu"), "AboutToShow",
                        &(0i32,),
                    ).await;
                }
                SniAction::MenuEvent { bus_name, menu_path, item_id } => {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as u32;
                    let data = zbus::zvariant::Value::I32(0);
                    let _ = conn_act.call_method(
                        Some(bus_name.as_str()), menu_path.as_str(),
                        Some("com.canonical.dbusmenu"), "Event",
                        &(item_id, "clicked", &data, timestamp),
                    ).await;
                }
                SniAction::FetchMenu { bus_name, menu_path, service_id } => {
                    let items2 = Arc::clone(&items_act);
                    let conn2  = conn_act.clone();
                    tokio::spawn(async move {
                        do_fetch_menu(&conn2, &bus_name, &menu_path, &service_id, items2).await;
                    });
                }
            }
        }
    });

    loop { tokio::time::sleep(tokio::time::Duration::from_secs(60)).await; }
}

// ============================================================================
// Watcher helpers
// ============================================================================

async fn try_become_watcher(items: TrayItems) -> Option<Connection> {
    let watcher = Watcher { items, registered: Mutex::new(Vec::new()) };
    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        async {
            ConnectionBuilder::session()?
                .name("org.kde.StatusNotifierWatcher")?
                .serve_at("/StatusNotifierWatcher", watcher)?
                .build().await
        },
    ).await;
    match result {
        Ok(Ok(conn)) => {
            let _ = conn.request_name("org.freedesktop.StatusNotifierWatcher").await;
            Some(conn)
        }
        Ok(Err(e)) => { eprintln!("SNI: watcher claim failed: {e}"); None }
        Err(_)     => { eprintln!("SNI: watcher claim timed out"); None }
    }
}

async fn query_watcher_items(conn: &Connection, watcher_name: &str) -> Vec<String> {
    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(3),
        conn.call_method(
            Some(watcher_name), "/StatusNotifierWatcher",
            Some("org.freedesktop.DBus.Properties"), "Get",
            &("org.kde.StatusNotifierWatcher", "RegisteredStatusNotifierItems"),
        ),
    ).await;
    let msg = match result {
        Ok(Ok(m))  => m,
        Ok(Err(_)) => return Vec::new(),   // watcher not present
        Err(_)     => return Vec::new(),   // timeout
    };
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
        Value::Value(inner) => match inner.as_ref() {
            Value::Array(a) => extract(a),
            _ => Vec::new(),
        },
        Value::Array(a) => extract(a),
        _ => Vec::new(),
    }
}

/// Returns true if the D-Bus error means this path is definitively not an SNI item.
/// Covers path-absent errors AND missing Properties interface (SNI requires Properties).
fn err_is_unknown_object(e: &zbus::Error) -> bool {
    let s = e.to_string();
    s.contains("UnknownObject")
        || s.contains("Unknown object")
        || s.contains("No such object path")
        || s.contains("does not exist at path")
        || (s.contains("No such interface") && s.contains("DBus.Properties"))
        || (s.contains("no such interface") && s.contains("DBus.Properties"))
}

/// True if Introspect XML declares any known SNI or AppIndicator interface.
fn xml_has_sni_interface(xml: &str) -> bool {
    xml.contains("<interface name=\"org.kde.StatusNotifierItem\"")
        || xml.contains("<interface name=\"org.ayatana.AppIndicator\"")
        || xml.contains("<interface name=\"org.freedesktop.StatusNotifierItem\"")
}

/// True if Introspect XML declares `org.freedesktop.DBus.Properties`.
/// Allows probing even when the SNI interface name is absent — some apps
/// (pasystray, GTK wrappers) implement SNI but don't advertise its name.
fn xml_has_properties_interface(xml: &str) -> bool {
    xml.contains("<interface name=\"org.freedesktop.DBus.Properties\"")
}

/// Extract `<node name="...">` child names from Introspect XML.
fn xml_child_names(xml: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in xml.lines() {
        let line = line.trim();
        if !line.starts_with("<node name=") { continue; }
        if let Some(s) = line.find('"') {
            let rest = &line[s + 1..];
            if let Some(e) = rest.find('"') {
                let name = &rest[..e];
                if !name.is_empty() && name != "/" {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

/// Probe a single unique bus name for SNI items using introspect-first strategy.
///
/// **Pass 1 — Ayatana / libappindicator**
///   Introspect `/org/ayatana/NotificationItem`. Each named child (e.g. "steam",
///   "nm_applet") is an SNI item. One round-trip finds all of them.
///
/// **Pass 2 — well-known SNI object paths**
///   Checks `/StatusNotifierItem` and `/org/kde/StatusNotifierItem`.
///   Gating rules:
///   - `UnknownObject` / no Properties iface / timeout → skip
///   - `UnknownMethod` (no Introspectable)             → try (cannot inspect XML)
///   - XML with SNI interface declared                  → try
///   - XML without SNI but with Properties interface    → try (pasystray-style)
///   - XML without both                                 → skip
///
/// **Pass 3 — deep introspect fallback**
///   Only runs when Pass 1+2 found nothing.  Walks the object tree looking
///   for any path that declares an SNI interface.
async fn scan_one_bus_name(conn: &Connection, bus_name: &str, items: TrayItems) {
    use tokio::time::{timeout, Duration};
    let t = Duration::from_secs(2);
    let mut found_any = false;

    // Pass 1: Ayatana parent — one Introspect call finds all libappindicator children.
    if let Ok(Ok(msg)) = timeout(t, conn.call_method(
        Some(bus_name), "/org/ayatana/NotificationItem",
        Some("org.freedesktop.DBus.Introspectable"), "Introspect", &(),
    )).await {
        let xml: String = msg.body().deserialize().unwrap_or_default();

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
    const SNI_PATHS: &[&str] = &[
        "/StatusNotifierItem",
        "/org/kde/StatusNotifierItem",
    ];
    for path in SNI_PATHS {
        let should_probe = match timeout(t, conn.call_method(
            Some(bus_name), *path,
            Some("org.freedesktop.DBus.Introspectable"), "Introspect", &(),
        )).await {
            Err(_)      => false,
            Ok(Err(e))  => !err_is_unknown_object(&e), // UnknownMethod → try
            Ok(Ok(msg)) => {
                let xml: String = msg.body().deserialize().unwrap_or_default();
                xml.is_empty()
                    || xml_has_sni_interface(&xml)
                    || xml_has_properties_interface(&xml)
            }
        };
        if should_probe {
            let svc = format!("{bus_name}{path}");
            if fetch_and_watch(conn, &svc, Arc::clone(&items)).await { found_any = true; }
        }
    }

    // Pass 3: deep introspect fallback — zero extra round-trips in the common case.
    if !found_any {
        if let Some(found_path) = introspect_find_sni_path(conn, bus_name).await {
            let svc = format!("{bus_name}{found_path}");
            fetch_and_watch(conn, &svc, Arc::clone(&items)).await;
        }
    }
}

/// Walk the D-Bus object tree to find the first path exposing an SNI interface.
/// Used as a last-resort fallback in `probe_service`. Checks up to 2 levels deep.
async fn introspect_find_sni_path(conn: &Connection, bus_name: &str) -> Option<String> {
    use tokio::time::{timeout, Duration};
    let t = Duration::from_secs(2);

    const ROOTS: &[&str] = &[
        "/StatusNotifierItem",
        "/org/ayatana/NotificationItem",
        "/org/kde/StatusNotifierItem",
        "/org",
        "/",
    ];

    for root in ROOTS {
        let Ok(Ok(msg)) = timeout(t, conn.call_method(
            Some(bus_name), *root,
            Some("org.freedesktop.DBus.Introspectable"), "Introspect", &(),
        )).await else { continue };

        let xml: String = msg.body().deserialize().unwrap_or_default();
        if xml.is_empty() { continue; }

        if xml_has_sni_interface(&xml) {
            eprintln!("SNI: introspect found {bus_name}{root}");
            return Some(root.to_string());
        }

        for child in xml_child_names(&xml) {
            let child_path = if *root == "/" { format!("/{child}") } else { format!("{root}/{child}") };
            let Ok(Ok(cm)) = timeout(t, conn.call_method(
                Some(bus_name), child_path.as_str(),
                Some("org.freedesktop.DBus.Introspectable"), "Introspect", &(),
            )).await else { continue };

            let cxml: String = cm.body().deserialize().unwrap_or_default();
            if xml_has_sni_interface(&cxml) {
                eprintln!("SNI: introspect found {bus_name}{child_path}");
                return Some(child_path);
            }

            for grandchild in xml_child_names(&cxml) {
                let gpath = format!("{child_path}/{grandchild}");
                let Ok(Ok(gm)) = timeout(t, conn.call_method(
                    Some(bus_name), gpath.as_str(),
                    Some("org.freedesktop.DBus.Introspectable"), "Introspect", &(),
                )).await else { continue };
                let gxml: String = gm.body().deserialize().unwrap_or_default();
                if xml_has_sni_interface(&gxml) {
                    eprintln!("SNI: introspect found {bus_name}{gpath}");
                    return Some(gpath);
                }
            }
        }
    }
    None
}

/// Probe a "bus_name/obj_path" service string as registered with the SNI watcher.
///
/// The service string may be either:
///   - "bus_name/obj_path"  (e.g. ":1.62/StatusNotifierItem")
///   - "bus_name"           (some watchers omit the path; default to /StatusNotifierItem)
///
/// Resolves well-known names to their unique owner before calling GetAll,
/// because Electron/Discord only respond to their unique name.
async fn probe_service(conn: &Connection, service: &str, items: TrayItems) {
    let (bus_name, obj_path) = split_service(service);

    // Resolve well-known name → unique name (:1.xxx) if needed.
    let unique = if bus_name.starts_with(':') {
        bus_name.to_string()
    } else {
        match resolve_unique_name(conn, bus_name).await {
            Some(u) => u,
            None    => {
                eprintln!("SNI: could not resolve {bus_name}");
                return;
            }
        }
    };

    let canonical = format!("{unique}{obj_path}");
    eprintln!("SNI: probing watcher item {canonical}");

    // Primary: try the exact path the watcher provided.
    let ok = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        fetch_and_watch(conn, &canonical, Arc::clone(&items)),
    ).await.unwrap_or(false);

    if ok { return; }

    // Fallback: if the watcher gave us just a bus name (no path), or the
    // canonical path failed, try introspecting to find the real SNI object.
    // This covers apps using libappindicator (where the path is a named child
    // of /org/ayatana/NotificationItem rather than /StatusNotifierItem).
    if let Some(found_path) = introspect_find_sni_path(conn, &unique).await {
        let found = format!("{unique}{found_path}");
        let _ = tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            fetch_and_watch(conn, &found, Arc::clone(&items)),
        ).await;
    }
}

// ============================================================================
// Icon fetching + per-item signal watcher
// ============================================================================

/// Fetch an icon, add it to `items`, then spawn a signal-watcher task so the
/// icon refreshes when the app emits `NewIcon` / `NewToolTip` / etc.
///
/// Returns `true` if a valid SNI item with a non-empty `Id` was found.
async fn fetch_and_watch(conn: &Connection, service: &str, items: TrayItems) -> bool {
    if fetch_icon(conn, service, Arc::clone(&items)).await {
        // Spawn a long-lived task that listens for property-change signals.
        let conn2   = conn.clone();
        let items2  = Arc::clone(&items);
        let service = service.to_string();
        tokio::spawn(async move {
            watch_sni_signals(&conn2, &service, items2).await;
        });
        true
    } else {
        false
    }
}

/// Build a D-Bus match rule for signals from `sender` on `interface`,
/// optionally filtering by `member`.
fn build_match_rule<'a>(
    sender:    &'a str,
    interface: &'a str,
    member:    Option<&'a str>,
) -> zbus::Result<zbus::MatchRule<'a>> {
    let mut b = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(sender)?
        .interface(interface)?;
    if let Some(m) = member {
        b = b.member(m)?;
    }
    Ok(b.build())
}

/// Subscribe to the SNI signals emitted by one item and re-fetch the icon
/// whenever something changes.
///
/// Uses `MessageStream::for_match_rule` (the zbus 4 API) to get two filtered
/// streams — one for `org.kde.StatusNotifierItem` signals and one for
/// `PropertiesChanged` — then selects across both.
async fn watch_sni_signals(conn: &Connection, service: &str, items: TrayItems) {
    let (bus_name, _obj_path) = split_service(service);
    let service_owned = service.to_string();

    // Build match rules using the typed builder API.
    // Filtering by sender alone is sufficient; each SNI item has its own
    // unique bus name so no cross-item leakage occurs.
    // NOTE: .path() is intentionally omitted — not available on all zbus 4 builds.
    let sni_rule = build_match_rule(bus_name, "org.kde.StatusNotifierItem", None);
    let sni_rule = match sni_rule {
        Ok(r) => r,
        Err(e) => { eprintln!("SNI: bad sni match rule: {e}"); return; }
    };

    let prop_rule = build_match_rule(
        bus_name,
        "org.freedesktop.DBus.Properties",
        Some("PropertiesChanged"),
    );
    let prop_rule = match prop_rule {
        Ok(r) => r,
        Err(e) => { eprintln!("SNI: bad prop match rule: {e}"); return; }
    };

    // Create filtered streams.  `None` for the buffer size uses the default.
    let sni_stream = match zbus::MessageStream::for_match_rule(
        sni_rule, conn, None,
    ).await {
        Ok(s)  => s,
        Err(e) => { eprintln!("SNI: for_match_rule (sni) failed: {e}"); return; }
    };

    let prop_stream = match zbus::MessageStream::for_match_rule(
        prop_rule, conn, None,
    ).await {
        Ok(s)  => s,
        Err(e) => { eprintln!("SNI: for_match_rule (props) failed: {e}"); return; }
    };

    // Merge the two streams so we can drive both with a single poll loop,
    // avoiding tokio::select! which requires the `macros` cargo feature.
    // Tag each event: true = SNI signal (carry member name), false = PropertiesChanged.
    let sni_tagged  = futures_util::StreamExt::map(sni_stream,  |r| (true,  r));
    let prop_tagged = futures_util::StreamExt::map(prop_stream, |r| (false, r));
    let mut merged  = futures_util::stream::select(sni_tagged, prop_tagged);

    while let Some((is_sni, result)) = futures_util::StreamExt::next(&mut merged).await {
        let member: Option<String> = match result {
            Err(_) => None,
            Ok(m) if is_sni => m.header().member().map(|n| n.as_str().to_string()),
            Ok(_)           => Some("PropertiesChanged".to_string()),
        };

        let should_refresh = matches!(
            member.as_deref(),
            Some("NewIcon")
            | Some("NewOverlayIcon")
            | Some("NewAttentionIcon")
            | Some("NewIconThemePath")
            | Some("NewStatus")
            | Some("NewToolTip")
            | Some("NewTitle")
            | Some("PropertiesChanged")
        );

        if should_refresh {
            eprintln!("SNI: refresh {service_owned} ({:?})", member);
            fetch_icon(conn, &service_owned, Arc::clone(&items)).await;
        }
    }
}

// ============================================================================
// Core icon fetching
// ============================================================================

type PropMap = HashMap<String, zbus::zvariant::OwnedValue>;

/// Resolve a well-known D-Bus name to its current unique owner name.
/// Returns `None` if the name has no owner or the call fails.
async fn resolve_unique_name(conn: &Connection, name: &str) -> Option<String> {
    let dbus = zbus::fdo::DBusProxy::new(conn).await.ok()?;
    dbus.get_name_owner(name.try_into().ok()?).await.ok()
        .map(|n| n.to_string())
}

/// GetAll filtered to `org.kde.StatusNotifierItem`.
async fn try_get_all_sni(props: &zbus::fdo::PropertiesProxy<'_>) -> PropMap {
    let iface = match zbus::names::InterfaceName::try_from("org.kde.StatusNotifierItem") {
        Ok(i) => i, Err(_) => return PropMap::new(),
    };
    props.get_all(zbus::zvariant::Optional::from(Some(iface))).await
        .unwrap_or_default()
}

/// GetAll filtered to `org.ayatana.AppIndicator`.
/// Some libappindicator apps (older nm-applet builds) only respond to this
/// interface name and return empty for `org.kde.StatusNotifierItem`.
async fn try_get_all_sni_ayatana(props: &zbus::fdo::PropertiesProxy<'_>) -> PropMap {
    let iface = match zbus::names::InterfaceName::try_from("org.ayatana.AppIndicator") {
        Ok(i) => i, Err(_) => return PropMap::new(),
    };
    props.get_all(zbus::zvariant::Optional::from(Some(iface))).await
        .unwrap_or_default()
}

/// GetAll with no interface filter — broad compat with Electron / libappindicator.
///
/// Sends `GetAll("")` as a raw D-Bus call so we definitely pass the empty string
/// on the wire, which some implementations require.  `Optional::from(None)` in
/// zbus may encode differently and some apps reject it.
async fn try_get_all_unfiltered(
    conn:     &Connection,
    bus_name: &str,
    obj_path: &str,
) -> PropMap {
    let msg = match conn.call_method(
        Some(bus_name), obj_path,
        Some("org.freedesktop.DBus.Properties"), "GetAll",
        &("",),  // empty string = all interfaces
    ).await {
        Ok(m)  => m,
        Err(e) => { eprintln!("SNI: GetAll(\"\") failed {bus_name}{obj_path}: {e}"); return PropMap::new(); }
    };

    let all: PropMap = match msg.body().deserialize() {
        Ok(a)  => a,
        Err(e) => { eprintln!("SNI: GetAll deserialize failed {bus_name}{obj_path}: {e}"); return PropMap::new(); }
    };

    eprintln!("SNI: GetAll(\"\") {bus_name}{obj_path} -> keys: {:?}", all.keys().collect::<Vec<_>>());

    // Accept only if mandatory SNI "Id" property is present.
    if all.contains_key("Id") { all } else { PropMap::new() }
}

/// GetAll filtered to `org.ayatana.AppIndicator` — for Ayatana / Ubuntu
/// libappindicator apps that don't respond to the KDE interface name.
async fn try_get_all_ayatana(
    conn:     &Connection,
    bus_name: &str,
    obj_path: &str,
) -> PropMap {
    let msg = match conn.call_method(
        Some(bus_name), obj_path,
        Some("org.freedesktop.DBus.Properties"), "GetAll",
        &("org.ayatana.AppIndicator",),
    ).await {
        Ok(m)  => m,
        Err(_) => return PropMap::new(),
    };

    let all: PropMap = match msg.body().deserialize() {
        Ok(a)  => a,
        Err(_) => return PropMap::new(),
    };

    eprintln!("SNI: GetAll(ayatana) {bus_name}{obj_path} -> keys: {:?}", all.keys().collect::<Vec<_>>());

    if all.contains_key("Id") { all } else { PropMap::new() }
}

/// Fetch each SNI property individually via `Get(interface, name)`.
///
/// This handles apps that implement `org.freedesktop.DBus.Properties` but
/// whose `GetAll` only accepts zero or no arguments (some old Ayatana/Qt
/// builds emit "Method GetAll with signature 's' doesn't exist").
/// We try both the KDE and Ayatana interface names.
async fn try_get_props_individually(
    conn:     &Connection,
    bus_name: &str,
    obj_path: &str,
) -> PropMap {
    use zbus::zvariant::Value;

    const INTERFACES: &[&str] = &[
        "org.kde.StatusNotifierItem",
        "org.ayatana.AppIndicator",
    ];
    const PROPS: &[&str] = &[
        "Id", "Category", "Status", "Title",
        "IconName", "IconThemePath",
        "IconPixmap",
        "AttentionIconName", "AttentionIconThemePath", "AttentionIconPixmap",
        "OverlayIconName", "OverlayIconPixmap",
        "ToolTip",
        "ItemIsMenu",
        "Menu",
    ];

    for iface in INTERFACES {
        // Quick probe: try fetching "Id" first.  If that fails this interface
        // is not supported and we move on.
        let id_result = conn.call_method(
            Some(bus_name), obj_path,
            Some("org.freedesktop.DBus.Properties"), "Get",
            &(iface, "Id"),
        ).await;
        eprintln!("SNI: Get({iface}, Id) at {bus_name}{obj_path} -> {}",
            match &id_result { Ok(_) => "ok".to_string(), Err(e) => format!("err: {e}") });
        let id_msg = match id_result {
            Ok(m)  => m,
            Err(_) => continue,
        };

        // The return type of Get is `v` (variant wrapping the real value).
        let id_val: zbus::zvariant::OwnedValue = match id_msg.body().deserialize() {
            Ok(v)  => v,
            Err(_) => continue,
        };
        let id_inner = match &*id_val {
            Value::Value(v) => zbus::zvariant::OwnedValue::try_from(v.as_ref()).ok(),
            _ => id_val.try_clone().ok(),
        };
        let id_str: Option<String> = id_inner.as_ref().and_then(|v| {
            if let Value::Str(s) = &**v { Some(s.to_string()) } else { None }
        });
        if id_str.as_deref().map_or(true, |s: &str| s.is_empty()) { continue; }

        // Id confirmed — fetch the rest.
        let mut map = PropMap::new();
        if let Some(val) = id_inner {
            map.insert("Id".to_string(), val);
        }

        for prop in PROPS.iter().filter(|&&p| p != "Id") {
            let Ok(msg) = conn.call_method(
                Some(bus_name), obj_path,
                Some("org.freedesktop.DBus.Properties"), "Get",
                &(iface, prop),
            ).await else { continue };

            let Ok(val): Result<zbus::zvariant::OwnedValue, _> = msg.body().deserialize()
            else { continue };

            // Unwrap the `v` wrapper returned by Get.
            let inner = match &*val {
                Value::Value(v) => zbus::zvariant::OwnedValue::try_from(v.as_ref()).ok(),
                _ => Some(val),
            };
            if let Some(inner) = inner {
                map.insert(prop.to_string(), inner);
            }
        }

        eprintln!("SNI: Get-individual {bus_name}{obj_path} (iface={iface}) -> keys: {:?}",
                  map.keys().collect::<Vec<_>>());
        return map;
    }

    PropMap::new()
}

/// Fetch properties from `service` ("bus_name/obj_path") and upsert into
/// `items`.  Returns `true` if a valid item with a non-empty `Id` was found.
///
/// Many apps (Electron, libappindicator) implement `GetAll` without honouring
/// the interface-name filter, returning an empty dict when a specific interface
/// is requested.  We therefore try two strategies in order:
///
/// 1. `GetAll("org.kde.StatusNotifierItem")` — the strictly-correct call.
/// 2. `GetAll("")` — no-filter; works with Discord, Electron, most GTK apps.
async fn fetch_icon(conn: &Connection, service: &str, items: TrayItems) -> bool {
    let (bus_name, obj_path) = split_service(service);

    let props = match zbus::fdo::PropertiesProxy::builder(conn)
        .destination(bus_name)
        .and_then(|b| b.path(obj_path))
        .map(|b| b.build())
    {
        Ok(f) => match f.await { Ok(p) => p, Err(_) => return false },
        Err(_) => return false,
    };

    // Resolve well-known names to their unique name (:1.xxx) before GetAll.
    // Discord and other Electron apps only respond to their unique name.
    let effective_bus = if bus_name.starts_with(':') {
        bus_name.to_string()
    } else {
        resolve_unique_name(conn, bus_name).await
            .unwrap_or_else(|| bus_name.to_string())
    };
    let effective_bus = effective_bus.as_str();

    // Fast-fail: introspect the object path before running GetAll strategies.
    //
    // This single round-trip tells us:
    //   - "UnknownObject"  → path missing entirely; skip all 4 strategies
    //   - "UnknownMethod"  → no Introspectable, but path may exist; continue
    //   - XML with SNI interface declared → confirmed SNI item; continue
    //   - XML without SNI interface → path exists but is something else; skip
    //
    // Saves 4+ redundant GetAll round-trips for the majority of non-SNI buses.
    let introspect_result = tokio::time::timeout(
        tokio::time::Duration::from_secs(2),
        conn.call_method(
            Some(effective_bus), obj_path,
            Some("org.freedesktop.DBus.Introspectable"), "Introspect", &(),
        ),
    ).await;

    match &introspect_result {
        Err(_) => return false,  // timeout
        Ok(Err(e)) if err_is_unknown_object(e) => return false,  // path does not exist
        Ok(Ok(msg)) => {
            // Path exists — check if SNI interface is declared.
            let xml: String = msg.body().deserialize().unwrap_or_default();
            if !xml.is_empty() && !xml_has_sni_interface(&xml) {
                // The path exists but is not an SNI item (e.g. an internal KDE
                // bus that happens to expose /StatusNotifierItem for something else).
                return false;
            }
            // xml is empty (unusual) or has SNI interface → proceed with GetAll.
        }
        Ok(Err(_)) => {} // UnknownMethod = no Introspectable → proceed with GetAll strategies
    }

    // Rebuild the PropertiesProxy with the resolved unique name.
    let props = if effective_bus != bus_name {
        match zbus::fdo::PropertiesProxy::builder(conn)
            .destination(effective_bus)
            .and_then(|b| b.path(obj_path))
            .map(|b| b.build())
        {
            Ok(f) => match f.await { Ok(p) => p, Err(_) => return false },
            Err(_) => return false,
        }
    } else {
        props
    };

    // Strategy 1a: GetAll("org.kde.StatusNotifierItem") — standard KDE/Qt/Electron.
    let mut all = try_get_all_sni(&props).await;
    if !all.is_empty() {
        eprintln!("SNI: GetAll(sni/kde) {service} -> {} keys", all.len());
    }

    // Strategy 1b: GetAll("org.ayatana.AppIndicator") — libappindicator apps.
    // Try this early because Ayatana items declare this interface in their XML.
    if all.is_empty() {
        all = try_get_all_sni_ayatana(&props).await;
        if !all.is_empty() {
            eprintln!("SNI: GetAll(sni/ayatana) {service} -> {} keys", all.len());
        }
    }

    // Strategy 2: unfiltered GetAll("") — Electron/Discord apps that return
    // empty dict when filtered by interface name but full props otherwise.
    if all.is_empty() {
        all = try_get_all_unfiltered(conn, effective_bus, obj_path).await;
    }

    // Strategy 3: GetAll("org.ayatana.AppIndicator") raw call — older Ubuntu
    // libappindicator builds that require the exact raw string interface name.
    if all.is_empty() {
        all = try_get_all_ayatana(conn, effective_bus, obj_path).await;
    }

    // Strategy 3b: GetAll with NO argument — some old Ayatana/Qt builds
    // implement GetAll() with signature "()" instead of "(s)".
    if all.is_empty() {
        match conn.call_method(
            Some(effective_bus), obj_path,
            Some("org.freedesktop.DBus.Properties"), "GetAll",
            &(),  // zero arguments
        ).await {
            Ok(msg) => {
                let candidate: PropMap = msg.body().deserialize().unwrap_or_default();
                eprintln!("SNI: GetAll() {effective_bus}{obj_path} -> keys: {:?}", candidate.keys().collect::<Vec<_>>());
                if candidate.contains_key("Id") {
                    all = candidate;
                }
            }
            Err(e) => eprintln!("SNI: GetAll() failed {effective_bus}{obj_path}: {e}"),
        }
    }

    // Strategy 4: individual Get(interface, property) calls — apps that
    // implement org.freedesktop.DBus.Properties but whose GetAll only
    // accepts zero arguments (old Qt/Ayatana builds).
    if all.is_empty() {
        eprintln!("SNI: trying Get-individual for {effective_bus}{obj_path}");
        all = try_get_props_individually(conn, effective_bus, obj_path).await;
    }

    // Still nothing means this path isn't an SNI item.
    if all.is_empty() { return false; }

    let id_str = match get_str(&all, "Id") {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };

    let icon_name       = get_str(&all, "IconName").filter(|s| !s.is_empty());
    let icon_theme_path = get_str(&all, "IconThemePath").filter(|s| !s.is_empty());
    let menu_path       = get_obj_path(&all, "Menu");

    // Status: Active | Passive | NeedsAttention (default Active)
    let status = match get_str(&all, "Status").as_deref() {
        Some("Passive")        => TrayStatus::Passive,
        Some("NeedsAttention") => TrayStatus::NeedsAttention,
        _                      => TrayStatus::Active,
    };

    // ItemIsMenu: when true left-click → ContextMenu(), not Activate()
    let item_is_menu = get_bool(&all, "ItemIsMenu");

    // Attention icon (shown when status == NeedsAttention)
    let attention_icon_name = get_str(&all, "AttentionIconName").filter(|s| !s.is_empty());
    let attention_pixmap    = all.get("AttentionIconPixmap").and_then(|v| parse_icon_pixmap(v));

    // Overlay icon (drawn on top of main icon)
    let overlay_icon_name = get_str(&all, "OverlayIconName").filter(|s| !s.is_empty());
    let overlay_pixmap    = all.get("OverlayIconPixmap").and_then(|v| parse_icon_pixmap(v));

    // ToolTip: spec property is a compound struct (s a(iiay) s s).
    // Fields: icon_name, icon_pixmaps, title, body.
    // Fall back through Title then Id for a usable display string.
    let (tooltip_title, tooltip_body) = parse_tooltip(&all)
        .unwrap_or_else(|| {
            let title = get_str(&all, "Title").filter(|s| !s.is_empty())
                .unwrap_or_else(|| id_str.clone());
            (title, String::new())
        });

    let pixmap = all.get("IconPixmap").and_then(|v| parse_icon_pixmap(v));

    eprintln!(
        "SNI: found {service}  id={id_str}  status={status:?}  item_is_menu={item_is_menu}          icon_name={icon_name:?}  theme={icon_theme_path:?}          pixmap={}  attn_icon={attention_icon_name:?}  menu={menu_path:?}",
        pixmap.as_ref().map_or_else(|| "none".to_string(), |(w,h,d)| format!("{w}x{h}({} bytes)", d.len()))
    );

    // Store the unique name in bus_name so signal watchers / menu calls work.
    // Discord and Electron apps only accept calls on their unique name.
    let icon = TrayIcon {
        id:              service.to_string(),
        bus_name:        effective_bus.to_string(),
        obj_path:        obj_path.to_string(),
        icon_rgba:       pixmap.as_ref().map(|(_, _, d)| d.clone()).unwrap_or_default(),
        icon_w:          pixmap.as_ref().map(|&(w, _, _)| w).unwrap_or(0),
        icon_h:          pixmap.as_ref().map(|&(_, h, _)| h).unwrap_or(0),
        icon_name,
        icon_theme_path,
        attention_icon_rgba: attention_pixmap.as_ref().map(|(_, _, d)| d.clone()).unwrap_or_default(),
        attention_icon_w:    attention_pixmap.as_ref().map(|&(w, _, _)| w).unwrap_or(0),
        attention_icon_h:    attention_pixmap.as_ref().map(|&(_, h, _)| h).unwrap_or(0),
        attention_icon_name,
        overlay_icon_rgba: overlay_pixmap.as_ref().map(|(_, _, d)| d.clone()).unwrap_or_default(),
        overlay_icon_w:    overlay_pixmap.as_ref().map(|&(w, _, _)| w).unwrap_or(0),
        overlay_icon_h:    overlay_pixmap.as_ref().map(|&(_, h, _)| h).unwrap_or(0),
        overlay_icon_name,
        status,
        item_is_menu,
        tooltip_title,
        tooltip_body,
        menu_path,
        menu_items:    Vec::new(),
        menu_revision: 0,
        menu_loaded:   false,
    };

    let mut locked = items.lock().unwrap();
    if let Some(existing) = locked.iter_mut().find(|i| i.id == icon.id) {
        // Preserve already-fetched menu data across icon refreshes.
        let menu_items    = existing.menu_items.clone();
        let menu_revision = existing.menu_revision;
        let menu_loaded   = existing.menu_loaded;
        *existing = icon;
        existing.menu_items    = menu_items;
        existing.menu_revision = menu_revision;
        existing.menu_loaded   = menu_loaded;
    } else {
        locked.push(icon);
    }

    true
}

// ============================================================================
// DBusMenu fetching
// ============================================================================

async fn do_fetch_menu(
    conn:       &Connection,
    bus_name:   &str,
    menu_path:  &str,
    service_id: &str,
    items:      TrayItems,
) {
    eprintln!("SNI: fetch_menu  bus={bus_name}  path={menu_path}  id={service_id}");

    // GetLayout(parentId=0, recursionDepth=-1, propertiesToGet=[])
    let result = conn.call_method(
        Some(bus_name), menu_path,
        Some("com.canonical.dbusmenu"), "GetLayout",
        &(0i32, -1i32, Vec::<String>::new()),
    ).await;

    let msg = match result {
        Ok(m)  => m,
        Err(e) => {
            eprintln!("SNI: GetLayout failed for {bus_name}{menu_path}: {e}");
            let mut locked = items.lock().unwrap();
            if let Some(icon) = locked.iter_mut().find(|i| i.id == service_id) {
                icon.menu_loaded = true;
            }
            return;
        }
    };

    // Response signature: (u(ia{sv}av))
    type MenuNodeRaw = (i32, HashMap<String, zbus::zvariant::OwnedValue>, Vec<zbus::zvariant::OwnedValue>);
    let (revision, root_node): (u32, MenuNodeRaw) = match msg.body().deserialize() {
        Ok(v)  => v,
        Err(e) => {
            eprintln!("SNI: GetLayout deserialize failed for {bus_name}: {e}");
            let mut locked = items.lock().unwrap();
            if let Some(icon) = locked.iter_mut().find(|i| i.id == service_id) {
                icon.menu_loaded = true;
            }
            return;
        }
    };

    let menu_items = parse_menu_items(&root_node.2);
    eprintln!("SNI: menu for {service_id}  rev={revision}  items={}", menu_items.len());

    let mut locked = items.lock().unwrap();
    if let Some(icon) = locked.iter_mut().find(|i| i.id == service_id) {
        icon.menu_items    = menu_items;
        icon.menu_revision = revision;
        icon.menu_loaded   = true;
    } else {
        eprintln!("SNI: fetch_menu — service_id '{service_id}' not found in tray items");
    }
}

fn parse_menu_items(children: &[zbus::zvariant::OwnedValue]) -> Vec<MenuItem> {
    use zbus::zvariant::Value;
    let mut items = Vec::new();

    for child_val in children {
        let inner: &Value = match &**child_val {
            Value::Value(boxed) => boxed.as_ref(),
            other               => other,
        };

        let child_struct = match inner {
            Value::Structure(s) => s,
            _ => continue,
        };

        let fields = child_struct.fields();
        if fields.len() < 3 { continue; }

        let id = match &fields[0] { Value::I32(v) => *v, _ => continue };

        let props: HashMap<String, String> = match &fields[1] {
            Value::Dict(d) => {
                d.iter().filter_map(|(k, v)| {
                    let key = match k { Value::Str(s) => s.to_string(), _ => return None };
                    let val = string_from_value(v)?;
                    Some((key, val))
                }).collect()
            }
            _ => HashMap::new(),
        };

        let is_separator = props.get("type").map(|t| t == "separator").unwrap_or(false);
        let label        = props.get("label").cloned().unwrap_or_default()
                               .replace('_', ""); // strip mnemonic underscores
        let enabled      = props.get("enabled").map(|v| v != "false").unwrap_or(true);
        let visible      = props.get("visible").map(|v| v != "false").unwrap_or(true);

        if !visible { continue; }

        let children_nested = match &fields[2] {
            Value::Array(a) => a.inner().iter().filter_map(|v| {
                zbus::zvariant::OwnedValue::try_from(v).ok()
            }).collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        let children = parse_menu_items(&children_nested);

        items.push(MenuItem { id, label, enabled, visible, is_separator, children });
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
// Helpers
// ============================================================================

fn split_service(service: &str) -> (&str, &str) {
    match service.find('/') {
        Some(pos) => (&service[..pos], &service[pos..]),
        None      => (service, "/StatusNotifierItem"),
    }
}

fn get_str(
    map: &HashMap<String, zbus::zvariant::OwnedValue>,
    key: &str,
) -> Option<String> {
    use zbus::zvariant::Value;
    match &**map.get(key)? {
        Value::Str(s) => Some(s.to_string()),
        _             => None,
    }
}

fn get_obj_path(
    map: &HashMap<String, zbus::zvariant::OwnedValue>,
    key: &str,
) -> Option<String> {
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

fn get_bool(
    map: &HashMap<String, zbus::zvariant::OwnedValue>,
    key: &str,
) -> bool {
    use zbus::zvariant::Value;
    match map.get(key).map(|v| &**v) {
        Some(Value::Bool(b))  => *b,
        Some(Value::Value(v)) => matches!(v.as_ref(), Value::Bool(true)),
        _                     => false,
    }
}

/// Parse the compound `ToolTip` property `(s a(iiay) s s)` into (title, body).
/// Returns `None` if the property is absent or unparseable.
fn parse_tooltip(
    map: &HashMap<String, zbus::zvariant::OwnedValue>,
) -> Option<(String, String)> {
    use zbus::zvariant::Value;

    let raw = map.get("ToolTip")?;

    // Unwrap outer variant wrapper if present.
    let inner: &Value = match &**raw {
        Value::Value(v) => v.as_ref(),
        other           => other,
    };

    let st = match inner {
        Value::Structure(s) => s,
        _ => return None,
    };
    let fields = st.fields();
    // Struct: (s a(iiay) s s) — icon_name, icon_pixmaps, title, body
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
        let st     = match item { Value::Structure(s) => s, _ => continue };
        let fields = st.fields();
        if fields.len() < 3 { continue; }

        let w = match &fields[0] { Value::I32(v) => *v as u32, _ => continue };
        let h = match &fields[1] { Value::I32(v) => *v as u32, _ => continue };
        let raw: Vec<u8> = match &fields[2] {
            Value::Array(a) => a.inner().iter().filter_map(|b| {
                if let Value::U8(byte) = b { Some(*byte) } else { None }
            }).collect(),
            _ => continue,
        };

        if raw.is_empty() || w == 0 || h == 0 { continue; }
        let rgba = argb_to_rgba(&raw);
        let area = w * h;
        if best.as_ref().map_or(true, |&(bw, bh, _)| area > bw * bh) {
            best = Some((w, h, rgba));
        }
    }

    best
}

fn argb_to_rgba(argb: &[u8]) -> Vec<u8> {
    argb.chunks_exact(4).flat_map(|c| [c[1], c[2], c[3], c[0]]).collect()
}