//! SNI (StatusNotifierItem) host + watcher + DBusMenu implementation.

use std::sync::{Arc, Mutex};
use std::thread;
use std::collections::HashMap;

use zbus::{interface, Connection, ConnectionBuilder};
use crate::gui::Config;

// ============================================================================
// Public types
// ============================================================================

#[derive(Clone, Debug, Default)]
pub struct MenuItem {
    pub id:           i32,
    pub label:        String,
    pub enabled:      bool,
    pub visible:      bool,
    pub is_separator: bool,
    pub children:     Vec<MenuItem>,
}

#[derive(Clone, Debug, Default)]
pub struct TrayIcon {
    pub id:              String,
    pub bus_name:        String,
    pub obj_path:        String,
    pub icon_rgba:       Vec<u8>,
    pub icon_w:          u32,
    pub icon_h:          u32,
    pub icon_name:       Option<String>,
    pub icon_theme_path: Option<String>,
    pub tooltip:         String,
    pub menu_path:       Option<String>,
    /// Menu layout fetched from DBusMenu; populated on first right-click.
    pub menu_items:      Vec<MenuItem>,
    pub menu_revision:   u32,
    /// Set to true once a GetLayout call has completed (even if it returned 0 items),
    /// so the GUI can distinguish "still loading" from "loaded but empty".
    pub menu_loaded:     bool,
}

pub type TrayItems = Arc<Mutex<Vec<TrayIcon>>>;

pub enum SniAction {
    Activate          { bus_name: String, obj_path: String },
    SecondaryActivate { bus_name: String, obj_path: String },
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

    pub fn secondary_activate(&self, bus_name: &str, obj_path: &str) {
        let _ = self.action_tx.send(SniAction::SecondaryActivate {
            bus_name: bus_name.to_string(),
            obj_path: obj_path.to_string(),
        });
    }

    /// Request menu layout fetch; result written back to TrayItems.
    pub fn fetch_menu(&self, bus_name: &str, menu_path: &str, service_id: &str) {
        let _ = self.action_tx.send(SniAction::FetchMenu {
            bus_name:   bus_name.to_string(),
            menu_path:  menu_path.to_string(),
            service_id: service_id.to_string(),
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

        eprintln!("SNI: registered {full}");
        let items = Arc::clone(&self.items);
        let conn  = conn.clone();
        tokio::spawn(async move { fetch_icon(&conn, &full, items).await; });
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
    let watcher = Watcher {
        items: Arc::clone(&items),
        registered: Mutex::new(Vec::new()),
    };

    let conn = ConnectionBuilder::session()?
        .name("org.kde.StatusNotifierWatcher")?
        .serve_at("/StatusNotifierWatcher", watcher)?
        .build()
        .await?;

    let host_name = format!("org.kde.StatusNotifierHost-{}", std::process::id());
    conn.request_name(host_name.as_str()).await?;

    let sig_ctx = zbus::SignalContext::new(&conn, "/StatusNotifierWatcher")?;
    let _ = Watcher::status_notifier_host_registered(&sig_ctx).await;

    // Active bus scan.
    {
        let dbus  = zbus::fdo::DBusProxy::new(&conn).await?;
        let names = dbus.list_names().await?;
        for name in &names {
            let name_str = name.as_str();
            if name_str.starts_with(':') { continue; }
            let key   = format!("{name_str}/StatusNotifierItem");
            let conn2 = conn.clone();
            let its2  = Arc::clone(&items);
            tokio::spawn(async move { fetch_icon(&conn2, &key, its2).await; });
        }
    }

    // Watch for new bus names.
    {
        let dbus        = zbus::fdo::DBusProxy::new(&conn).await?;
        let mut stream  = dbus.receive_name_owner_changed().await?;
        let conn_w      = conn.clone();
        let items_w     = Arc::clone(&items);
        tokio::spawn(async move {
            while let Some(sig) = futures_util::StreamExt::next(&mut stream).await {
                if let Ok(args) = sig.args() {
                    let name = args.name().to_string();
                    if args.new_owner().is_some() && !name.starts_with(':') {
                        let key  = format!("{name}/StatusNotifierItem");
                        let c2   = conn_w.clone();
                        let its2 = Arc::clone(&items_w);
                        tokio::spawn(async move { fetch_icon(&c2, &key, its2).await; });
                    }
                }
            }
        });
    }

    // Action handler.
    let conn_act  = conn.clone();
    let items_act = Arc::clone(&items);
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            match action {
                SniAction::Activate { bus_name, obj_path } => {
                    let _ = conn_act.call_method(
                        Some(bus_name.as_str()), obj_path.as_str(),
                        Some("org.kde.StatusNotifierItem"), "Activate", &(0i32, 0i32),
                    ).await;
                }
                SniAction::SecondaryActivate { bus_name, obj_path } => {
                    let _ = conn_act.call_method(
                        Some(bus_name.as_str()), obj_path.as_str(),
                        Some("org.kde.StatusNotifierItem"), "SecondaryActivate", &(0i32, 0i32),
                    ).await;
                }
                SniAction::MenuAboutToShow { bus_name, menu_path } => {
                    let _ = conn_act.call_method(
                        Some(bus_name.as_str()), menu_path.as_str(),
                        Some("com.canonical.dbusmenu"), "AboutToShow", &(0i32,),
                    ).await;
                }
                SniAction::MenuEvent { bus_name, menu_path, item_id } => {
                    // Event types: "clicked", timestamp
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
                        fetch_menu(&conn2, &bus_name, &menu_path, &service_id, items2).await;
                    });
                }
            }
        }
    });

    loop { tokio::time::sleep(tokio::time::Duration::from_secs(60)).await; }
}

// ============================================================================
// Icon fetching
// ============================================================================

async fn fetch_icon(conn: &Connection, service: &str, items: TrayItems) {
    let (bus_name, obj_path) = split_service(service);

    let props = match zbus::fdo::PropertiesProxy::builder(conn)
        .destination(bus_name)
        .and_then(|b| b.path(obj_path))
        .map(|b| b.build())
    {
        Ok(f) => match f.await { Ok(p) => p, Err(_) => return },
        Err(_) => return,
    };

    let iface = match zbus::names::InterfaceName::try_from("org.kde.StatusNotifierItem") {
        Ok(i) => i, Err(_) => return,
    };

    let all = match props.get_all(zbus::zvariant::Optional::from(Some(iface))).await {
        Ok(a) => a, Err(_) => return,
    };

    let id_str = match get_str(&all, "Id") {
        Some(s) if !s.is_empty() => s,
        _ => return,
    };

    let icon_name       = get_str(&all, "IconName").filter(|s| !s.is_empty());
    let icon_theme_path = get_str(&all, "IconThemePath").filter(|s| !s.is_empty());
    let menu_path       = get_obj_path(&all, "Menu");

    let tooltip = get_str(&all, "ToolTipTitle").filter(|s| !s.is_empty())
        .or_else(|| get_str(&all, "Title").filter(|s| !s.is_empty()))
        .unwrap_or_else(|| id_str.clone());

    eprintln!("SNI: found {service}  id={id_str}  icon_name={icon_name:?}  menu={menu_path:?}");

    let pixmap = all.get("IconPixmap").and_then(|v| parse_icon_pixmap(v));

    let icon = TrayIcon {
        id:              service.to_string(),
        bus_name:        bus_name.to_string(),
        obj_path:        obj_path.to_string(),
        icon_rgba:       pixmap.as_ref().map(|(_, _, d)| d.clone()).unwrap_or_default(),
        icon_w:          pixmap.as_ref().map(|&(w, _, _)| w).unwrap_or(0),
        icon_h:          pixmap.as_ref().map(|&(_, h, _)| h).unwrap_or(0),
        icon_name,
        icon_theme_path,
        tooltip,
        menu_path,
        menu_items:    Vec::new(),
        menu_revision: 0,
        menu_loaded:   false,
    };

    let mut locked = items.lock().unwrap();
    if let Some(existing) = locked.iter_mut().find(|i| i.id == icon.id) {
        // Preserve menu if already fetched.
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
}

// ============================================================================
// DBusMenu fetching
// ============================================================================

async fn fetch_menu(
    conn:       &Connection,
    bus_name:   &str,
    menu_path:  &str,
    service_id: &str,
    items:      TrayItems,
) {
    eprintln!("SNI: fetch_menu start  bus={bus_name}  path={menu_path}  id={service_id}");

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
            // Mark loaded so the GUI stops showing "Loading…".
            let mut locked = items.lock().unwrap();
            if let Some(icon) = locked.iter_mut().find(|i| i.id == service_id) {
                icon.menu_loaded = true;
            }
            return;
        }
    };

    // Response signature: (u(ia{sv}av))
    // Deserialise as concrete Rust types to avoid fragile OwnedValue tree-walking.
    type MenuNodeRaw = (i32, std::collections::HashMap<String, zbus::zvariant::OwnedValue>, Vec<zbus::zvariant::OwnedValue>);
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
        // Each child is an OwnedValue; unwrap one level of Value::Value wrapping if present.
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

        // Parse a{sv} property dict.
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

        // Recurse: grandchildren are in fields[2] as av (Array of Variant/Structure).
        let children_nested = match &fields[2] {
            Value::Array(a) => {
                // av → convert each element to OwnedValue for recursion.
                a.inner().iter().filter_map(|v| {
                    zbus::zvariant::OwnedValue::try_from(v.clone()).ok()
                }).collect::<Vec<_>>()
            }
            _ => Vec::new(),
        };
        let children = parse_menu_items(&children_nested);

        items.push(MenuItem { id, label, enabled, visible, is_separator, children });
    }

    items
}

/// Extract a display string from a Value (handles Variant wrapping).
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

fn parse_icon_pixmap(val: &zbus::zvariant::OwnedValue) -> Option<(u32, u32, Vec<u8>)> {
    use zbus::zvariant::Value;

    let arr = match &**val { Value::Array(a) => a, _ => return None };
    let mut best: Option<(u32, u32, Vec<u8>)> = None;

    for item in arr.inner() {
        let st = match item { Value::Structure(s) => s, _ => continue };
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