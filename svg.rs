/// Minimal SVG rasterizer — no external XML or SVG crates.
///
/// XML parsing: tiny hand-rolled recursive-descent parser (~80 lines).
/// Rasterization: tiny-skia (already a transitive dep via eframe/egui's
/// software renderer; now made direct and explicit).
///
/// Covers the ~95% of icon SVGs that use shapes, fills, strokes, opacity,
/// linear/radial gradients, transforms, <use>/<defs>, and group nesting.
/// Text, filters, clip-paths, masks, and patterns are intentionally skipped
/// (they never appear in icon sets).

use std::collections::HashMap;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, Path,
    PathBuilder, Pixmap, Point, RadialGradient, Rect, Shader,
    SpreadMode, Stroke, Transform,
};

// ── public entry point ───────────────────────────────────────────────────────

/// Rasterize `svg_bytes` at the SVG's natural size, returning (rgba, w, h).
/// Falls back to `fallback_px × fallback_px` when size cannot be determined.
pub fn rasterize(
    svg_bytes: &[u8],
    fallback_px: u32,
) -> Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
    let text = std::str::from_utf8(svg_bytes)?;
    let root  = parse_xml(text).ok_or("SVG parse error")?;

    let (vb_x, vb_y, vb_w, vb_h) = parse_viewbox(&root);
    let w = attr_f32(&root, "width",  vb_w).round() as u32;
    let h = attr_f32(&root, "height", vb_h).round() as u32;
    let (w, h) = (w.max(1).min(4096), h.max(1).min(4096));
    let (w, h) = if w == 0 || h == 0 { (fallback_px, fallback_px) } else { (w, h) };

    let sx = w as f32 / vb_w.max(1.0);
    let sy = h as f32 / vb_h.max(1.0);
    let root_tx = Transform::from_translate(-vb_x * sx, -vb_y * sy).post_scale(sx, sy);

    let mut pixmap = Pixmap::new(w, h).ok_or("bad pixmap size")?;
    let defs = collect_defs(&root);
    let ctx  = Ctx { defs: &defs, pw: w as f32, ph: h as f32 };

    render_children(&root, &ctx, &mut pixmap, root_tx, &Props::default());
    Ok((pixmap.data().to_vec(), w, h))
}

// ── minimal XML tree ─────────────────────────────────────────────────────────

struct El {
    name:     String,                   // local name (namespace prefix stripped)
    attrs:    Vec<(String, String)>,    // (local name, decoded value)
    children: Vec<El>,
}

impl El {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    fn attr2(&self, a: &str, b: &str) -> Option<&str> { self.attr(a).or_else(|| self.attr(b)) }
}

// ── XML parser ───────────────────────────────────────────────────────────────

fn parse_xml(src: &str) -> Option<El> {
    let bytes = src.as_bytes();
    let mut pos = 0;
    // Skip until we hit the root element (skip <?xml?>, <!DOCTYPE>, comments)
    loop {
        skip_whitespace(bytes, &mut pos);
        if pos >= bytes.len() { return None; }
        if bytes[pos] != b'<' { pos += 1; continue; }
        pos += 1;
        if pos >= bytes.len() { return None; }
        match bytes[pos] {
            b'?' => { skip_to(bytes, &mut pos, b'>'); continue; }  // <?...?>
            b'!' => { skip_to(bytes, &mut pos, b'>'); continue; }  // <!-- --> / <!DOCTYPE>
            b'/' => return None,                                    // stray close tag
            _    => { pos -= 1; return parse_element(bytes, &mut pos); }
        }
    }
}

fn parse_element(bytes: &[u8], pos: &mut usize) -> Option<El> {
    if bytes.get(*pos) != Some(&b'<') { return None; }
    *pos += 1;

    let name = local_name(read_name(bytes, pos));
    skip_whitespace(bytes, pos);

    let mut attrs: Vec<(String, String)> = Vec::new();
    loop {
        skip_whitespace(bytes, pos);
        match bytes.get(*pos) {
            None | Some(&b'>') => { *pos += 1; break; }
            Some(&b'/') => {
                // Self-closing: skip '/>'
                while *pos < bytes.len() && bytes[*pos] != b'>' { *pos += 1; }
                *pos += 1;
                return Some(El { name, attrs, children: Vec::new() });
            }
            _ => {
                let key = local_name(read_name(bytes, pos));
                skip_whitespace(bytes, pos);
                if bytes.get(*pos) == Some(&b'=') {
                    *pos += 1;
                    skip_whitespace(bytes, pos);
                    let val = read_attr_value(bytes, pos);
                    if !key.is_empty() { attrs.push((key, val)); }
                } else if !key.is_empty() {
                    attrs.push((key, String::new()));
                }
            }
        }
    }

    // Parse children
    let mut children: Vec<El> = Vec::new();
    loop {
        skip_whitespace(bytes, pos);
        if *pos >= bytes.len() { break; }
        if bytes[*pos] != b'<' { skip_to(bytes, pos, b'<'); continue; }
        *pos += 1;
        if *pos >= bytes.len() { break; }
        match bytes[*pos] {
            b'/' => {
                // Close tag — consume </name>
                skip_to(bytes, pos, b'>');
                *pos += 1;
                break;
            }
            b'!' | b'?' => { skip_to(bytes, pos, b'>'); }  // comments / PI
            _ => {
                *pos -= 1;
                if let Some(child) = parse_element(bytes, pos) {
                    children.push(child);
                }
            }
        }
    }
    Some(El { name, attrs, children })
}

// Read an XML name (tag or attribute name).
fn read_name<'a>(bytes: &'a [u8], pos: &mut usize) -> &'a str {
    let start = *pos;
    while *pos < bytes.len() {
        let b = bytes[*pos];
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b':' {
            *pos += 1;
        } else { break; }
    }
    std::str::from_utf8(&bytes[start..*pos]).unwrap_or("")
}

// Read a quoted attribute value, decode basic XML entities.
fn read_attr_value(bytes: &[u8], pos: &mut usize) -> String {
    let quote = match bytes.get(*pos) { Some(&b'"') | Some(&b'\'') => bytes[*pos], _ => b'"' };
    *pos += 1;
    let mut out = String::new();
    while *pos < bytes.len() {
        let b = bytes[*pos];
        if b == quote { *pos += 1; break; }
        if b == b'&' {
            *pos += 1;
            let start = *pos;
            while *pos < bytes.len() && bytes[*pos] != b';' { *pos += 1; }
            let ent = std::str::from_utf8(&bytes[start..*pos]).unwrap_or("");
            out.push_str(match ent {
                "amp" => "&", "lt" => "<", "gt" => ">", "quot" => "\"", "apos" => "'", _ => "",
            });
            *pos += 1;
        } else {
            out.push(b as char);
            *pos += 1;
        }
    }
    out
}

fn skip_whitespace(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && (bytes[*pos] == b' ' || bytes[*pos] == b'\t'
                               || bytes[*pos] == b'\n' || bytes[*pos] == b'\r') {
        *pos += 1;
    }
}

fn skip_to(bytes: &[u8], pos: &mut usize, target: u8) {
    while *pos < bytes.len() && bytes[*pos] != target { *pos += 1; }
}

/// Strip namespace prefix: "svg:path" → "path", "xlink:href" → "href"
fn local_name(s: &str) -> String {
    s.rsplit(':').next().unwrap_or(s).to_string()
}

// ── defs collection ───────────────────────────────────────────────────────────

struct Defs<'a> {
    gradients: HashMap<&'a str, &'a El>,
    symbols:   HashMap<&'a str, &'a El>,
    elements:  HashMap<&'a str, &'a El>,
}

fn collect_defs(root: &El) -> Defs<'_> {
    let mut d = Defs { gradients: HashMap::new(), symbols: HashMap::new(), elements: HashMap::new() };
    collect_defs_rec(root, &mut d);
    d
}

fn collect_defs_rec<'a>(el: &'a El, d: &mut Defs<'a>) {
    for child in &el.children {
        if let Some(id) = child.attr("id") {
            match child.name.as_str() {
                "linearGradient" | "radialGradient" => { d.gradients.insert(id, child); }
                "symbol"                            => { d.symbols.insert(id, child); }
                _                                   => { d.elements.insert(id, child); }
            }
        }
        collect_defs_rec(child, d);
    }
}

// ── rendering context ─────────────────────────────────────────────────────────

struct Ctx<'a> { defs: &'a Defs<'a>, pw: f32, ph: f32 }

/// Heritable presentation properties — resolved from ancestor + inline style.
#[derive(Clone)]
struct Props {
    fill:           SvgPaint,
    fill_opacity:   f32,
    fill_rule:      FillRule,
    stroke:         SvgPaint,
    stroke_opacity: f32,
    stroke_width:   f32,
    opacity:        f32,
}

impl Default for Props {
    fn default() -> Self {
        Props {
            fill: SvgPaint::Color(0, 0, 0), fill_opacity: 1.0, fill_rule: FillRule::Winding,
            stroke: SvgPaint::None, stroke_opacity: 1.0, stroke_width: 1.0, opacity: 1.0,
        }
    }
}

#[derive(Clone)]
enum SvgPaint { None, Color(u8, u8, u8), Url(String) }

// ── tree walk ─────────────────────────────────────────────────────────────────

fn render_children(parent: &El, ctx: &Ctx, pm: &mut Pixmap, tx: Transform, props: &Props) {
    for child in &parent.children { render_node(child, ctx, pm, tx, props); }
}

fn render_node(el: &El, ctx: &Ctx, pm: &mut Pixmap, tx: Transform, props: &Props) {
    if is_hidden(el) { return; }

    if el.name == "use" { render_use(el, ctx, pm, tx, props); return; }

    let ltx   = el.attr("transform").and_then(parse_transform)
                  .map(|t| tx.post_concat(t)).unwrap_or(tx);
    let lprops = inherit_props(props, el);

    match el.name.as_str() {
        "g" | "svg" | "symbol" => render_children(el, ctx, pm, ltx, &lprops),
        "path"     => { if let Some(p) = el.attr("d").and_then(parse_path_d) { paint(&p, ctx, pm, ltx, &lprops); } }
        "rect"     => render_rect(el, ctx, pm, ltx, &lprops),
        "circle"   => render_circle(el, ctx, pm, ltx, &lprops),
        "ellipse"  => render_ellipse(el, ctx, pm, ltx, &lprops),
        "line"     => render_line(el, ctx, pm, ltx, &lprops),
        "polyline" => render_poly(el, false, ctx, pm, ltx, &lprops),
        "polygon"  => render_poly(el, true,  ctx, pm, ltx, &lprops),
        "defs" | "title" | "desc" | "metadata" | "style" => {}
        _ => render_children(el, ctx, pm, ltx, &lprops),
    }
}

fn render_use(el: &El, ctx: &Ctx, pm: &mut Pixmap, tx: Transform, props: &Props) {
    let href = el.attr2("href", "xlink:href").unwrap_or("").trim_start_matches('#');
    if href.is_empty() { return; }
    let dx = parse_f32(el.attr("x").unwrap_or("0"));
    let dy = parse_f32(el.attr("y").unwrap_or("0"));
    let ltx = tx.post_concat(Transform::from_translate(dx, dy));
    let lp  = inherit_props(props, el);
    if let Some(t) = ctx.defs.symbols.get(href) { render_children(t, ctx, pm, ltx, &lp); }
    else if let Some(t) = ctx.defs.elements.get(href) { render_node(t, ctx, pm, ltx, &lp); }
}

// ── shapes ────────────────────────────────────────────────────────────────────

fn render_rect(el: &El, ctx: &Ctx, pm: &mut Pixmap, tx: Transform, props: &Props) {
    let (x, y) = (af(el,"x"), af(el,"y"));
    let (w, h) = (af(el,"width"), af(el,"height"));
    if w <= 0.0 || h <= 0.0 { return; }
    let rx = el.attr2("rx","ry").map(parse_f32).unwrap_or(0.0).min(w/2.0);
    let ry = el.attr2("ry","rx").map(parse_f32).unwrap_or(0.0).min(h/2.0);
    let path = if rx == 0.0 && ry == 0.0 {
        Rect::from_xywh(x,y,w,h).map(|r| PathBuilder::from_rect(r))
    } else {
        Some(rounded_rect(x,y,w,h,rx,ry))
    };
    if let Some(p) = path { paint(&p, ctx, pm, tx, props); }
}

fn render_circle(el: &El, ctx: &Ctx, pm: &mut Pixmap, tx: Transform, props: &Props) {
    let r = af(el,"r"); if r <= 0.0 { return; }
    if let Some(p) = PathBuilder::from_circle(af(el,"cx"), af(el,"cy"), r) { paint(&p, ctx, pm, tx, props); }
}

fn render_ellipse(el: &El, ctx: &Ctx, pm: &mut Pixmap, tx: Transform, props: &Props) {
    let (rx, ry) = (af(el,"rx"), af(el,"ry")); if rx <= 0.0 || ry <= 0.0 { return; }
    paint(&ellipse_path(af(el,"cx"), af(el,"cy"), rx, ry), ctx, pm, tx, props);
}

fn render_line(el: &El, ctx: &Ctx, pm: &mut Pixmap, tx: Transform, props: &Props) {
    let mut pb = PathBuilder::new();
    pb.move_to(af(el,"x1"), af(el,"y1"));
    pb.line_to(af(el,"x2"), af(el,"y2"));
    if let Some(p) = pb.finish() { paint(&p, ctx, pm, tx, props); }
}

fn render_poly(el: &El, close: bool, ctx: &Ctx, pm: &mut Pixmap, tx: Transform, props: &Props) {
    let pts: Vec<f32> = el.attr("points").unwrap_or("").split(|c: char| c==',' || c.is_whitespace())
        .filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    if pts.len() < 4 { return; }
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0], pts[1]);
    for ch in pts.chunks_exact(2).skip(1) { pb.line_to(ch[0], ch[1]); }
    if close { pb.close(); }
    if let Some(p) = pb.finish() { paint(&p, ctx, pm, tx, props); }
}

// ── painting ──────────────────────────────────────────────────────────────────

fn paint(path: &Path, ctx: &Ctx, pm: &mut Pixmap, tx: Transform, props: &Props) {
    if !matches!(props.fill, SvgPaint::None) {
        let a = alpha(props.fill_opacity, props.opacity);
        if a > 0 { if let Some(p) = mk_paint(&props.fill, a, ctx) { pm.fill_path(path, &p, props.fill_rule, tx, None); } }
    }
    if !matches!(props.stroke, SvgPaint::None) && props.stroke_width > 0.0 {
        let a = alpha(props.stroke_opacity, props.opacity);
        if a > 0 { if let Some(p) = mk_paint(&props.stroke, a, ctx) {
            pm.stroke_path(path, &p, &Stroke { width: props.stroke_width, ..Default::default() }, tx, None);
        }}
    }
}

fn alpha(op1: f32, op2: f32) -> u8 { (op1 * op2 * 255.0).round().clamp(0.0, 255.0) as u8 }

fn mk_paint(sp: &SvgPaint, a: u8, ctx: &Ctx) -> Option<Paint<'static>> {
    match sp {
        SvgPaint::None => None,
        SvgPaint::Color(r,g,b) => {
            let mut p = Paint::default(); p.anti_alias = true;
            p.shader = Shader::SolidColor(Color::from_rgba8(*r,*g,*b,a)); Some(p)
        }
        SvgPaint::Url(id) => resolve_gradient(id, a, ctx),
    }
}

// ── gradients ─────────────────────────────────────────────────────────────────

fn resolve_gradient(id: &str, a: u8, ctx: &Ctx) -> Option<Paint<'static>> {
    let node = ctx.defs.gradients.get(id)?;
    // Follow one level of href chaining for shared stops
    let base = node.attr2("href","xlink:href")
        .map(|h| h.trim_start_matches('#'))
        .and_then(|hid| ctx.defs.gradients.get(hid).copied())
        .unwrap_or(node);

    let stops = collect_stops(base, a);
    if stops.is_empty() { return None; }

    let gt = node.attr("gradientTransform").and_then(parse_transform).unwrap_or(Transform::identity());

    match node.name.as_str() {
        "linearGradient" => {
            let (x1,y1) = (gl(node,"x1",0.0,ctx.pw), gl(node,"y1",0.0,ctx.ph));
            let (x2,y2) = (gl(node,"x2",1.0,ctx.pw), gl(node,"y2",0.0,ctx.ph));
            let g = LinearGradient::new(Point::from_xy(x1,y1), Point::from_xy(x2,y2), stops, SpreadMode::Pad, gt)?;
            let mut p = Paint::default(); p.anti_alias = true; p.shader = g; Some(p)
        }
        "radialGradient" => {
            let dim = ctx.pw.max(ctx.ph);
            let (cx,cy) = (gl(node,"cx",0.5,ctx.pw), gl(node,"cy",0.5,ctx.ph));
            let r        = gl(node,"r", 0.5, dim);
            let (fx,fy) = (gl(node,"fx",cx, ctx.pw), gl(node,"fy",cy, ctx.ph));
            // tiny-skia 0.12: RadialGradient::new(start_point, start_radius, end_point, end_radius, ...)
            // SVG mapping: start = focal point (fx,fy) r=0, end = center (cx,cy) r=r
            let g = RadialGradient::new(Point::from_xy(fx,fy), 0.0, Point::from_xy(cx,cy), r, stops, SpreadMode::Pad, gt)?;
            let mut p = Paint::default(); p.anti_alias = true; p.shader = g; Some(p)
        }
        _ => None,
    }
}

/// Resolve a gradient length attribute that may be a fraction (0–1), percentage, or absolute.
fn gl(el: &El, attr: &str, default_frac: f32, dim: f32) -> f32 {
    match el.attr(attr) {
        None => default_frac * dim,
        Some(s) if s.ends_with('%') => s.trim_end_matches('%').parse::<f32>().unwrap_or(default_frac*100.0) / 100.0 * dim,
        Some(s) => s.parse::<f32>().unwrap_or(default_frac * dim),
    }
}

fn collect_stops(el: &El, a: u8) -> Vec<GradientStop> {
    // Collect with offset tracked separately (GradientStop.position is pub(crate))
    let mut tagged: Vec<(f32, GradientStop)> = el.children.iter()
        .filter(|n| n.name == "stop")
        .map(|s| {
            let offset = parse_stop_offset(s.attr("offset").unwrap_or("0"));
            let (r, g, b, sa) = stop_color(s);
            let combined = ((sa as u16 * a as u16) / 255) as u8;
            (offset, GradientStop::new(offset, Color::from_rgba8(r, g, b, combined)))
        })
        .collect();
    tagged.sort_by(|a,b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    tagged.into_iter().map(|(_, s)| s).collect()
}

fn parse_stop_offset(s: &str) -> f32 {
    if s.ends_with('%') { s.trim_end_matches('%').parse::<f32>().unwrap_or(0.0) / 100.0 }
    else { s.parse::<f32>().unwrap_or(0.0) }.clamp(0.0, 1.0)
}

fn stop_color(stop: &El) -> (u8, u8, u8, u8) {
    let style = stop.attr("style").unwrap_or("");
    let color_src = style_prop(style,"stop-color").or_else(|| stop.attr("stop-color")).unwrap_or("black");
    let op_src    = style_prop(style,"stop-opacity").or_else(|| stop.attr("stop-opacity")).unwrap_or("1");
    let (r,g,b)   = parse_color(color_src).unwrap_or((0,0,0));
    let alpha     = (op_src.parse::<f32>().unwrap_or(1.0).clamp(0.0,1.0) * 255.0).round() as u8;
    (r, g, b, alpha)
}

// ── props inheritance ─────────────────────────────────────────────────────────

fn inherit_props(parent: &Props, el: &El) -> Props {
    let s = el.attr("style").unwrap_or("");
    let mut p = parent.clone();
    macro_rules! get { ($css:expr, $attr:expr) => { style_prop(s,$css).or_else(|| el.attr($attr)) } }

    if let Some(v) = get!("fill","fill")                  { p.fill          = parse_paint(v, &parent.fill); }
    if let Some(v) = get!("fill-opacity","fill-opacity")  { p.fill_opacity  = v.parse::<f32>().unwrap_or(p.fill_opacity).clamp(0.0,1.0); }
    if let Some(v) = get!("fill-rule","fill-rule")        { p.fill_rule     = if v=="evenodd" { FillRule::EvenOdd } else { FillRule::Winding }; }
    if let Some(v) = get!("stroke","stroke")              { p.stroke        = parse_paint(v, &parent.stroke); }
    if let Some(v) = get!("stroke-opacity","stroke-opacity") { p.stroke_opacity = v.parse::<f32>().unwrap_or(p.stroke_opacity).clamp(0.0,1.0); }
    if let Some(v) = get!("stroke-width","stroke-width")  {
        p.stroke_width = v.trim_end_matches("px").parse::<f32>().unwrap_or(p.stroke_width).max(0.0);
    }
    if let Some(v) = get!("opacity","opacity") {
        p.opacity = parent.opacity * v.parse::<f32>().unwrap_or(1.0).clamp(0.0,1.0);
    }
    p
}

fn parse_paint(s: &str, _inherit: &SvgPaint) -> SvgPaint {
    let s = s.trim();
    if s == "none"         { return SvgPaint::None; }
    if s == "currentColor" { return SvgPaint::Color(0,0,0); }
    if let Some(id) = s.strip_prefix("url(#").and_then(|t| t.strip_suffix(')')) {
        return SvgPaint::Url(id.to_string());
    }
    parse_color(s).map(|(r,g,b)| SvgPaint::Color(r,g,b)).unwrap_or(SvgPaint::None)
}

fn style_prop<'a>(style: &'a str, prop: &str) -> Option<&'a str> {
    style.split(';').find_map(|d| {
        let (k,v) = d.split_once(':')?;
        (k.trim() == prop).then(|| v.trim())
    })
}

// ── colour parsing ────────────────────────────────────────────────────────────

fn parse_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') { return parse_hex(h); }
    if let Some(b) = s.strip_prefix("rgb(").and_then(|t| t.strip_suffix(')'))  { return parse_rgb(b); }
    if let Some(b) = s.strip_prefix("rgba(").and_then(|t| t.strip_suffix(')')) { return parse_rgb(b); }
    named(s)
}

fn parse_hex(h: &str) -> Option<(u8, u8, u8)> {
    let x = |s: &str| u8::from_str_radix(s, 16).ok();
    match h.len() {
        3|4 => Some((x(&h[0..1])? * 17, x(&h[1..2])? * 17, x(&h[2..3])? * 17)),
        6|8 => Some((x(&h[0..2])?, x(&h[2..4])?, x(&h[4..6])?)),
        _ => None,
    }
}

fn parse_rgb(b: &str) -> Option<(u8, u8, u8)> {
    let ch = |s: &str| -> Option<u8> {
        let s = s.trim();
        if s.ends_with('%') { Some((s.trim_end_matches('%').parse::<f32>().ok()? / 100.0 * 255.0).round() as u8) }
        else { Some(s.parse::<f32>().ok()?.round().clamp(0.0,255.0) as u8) }
    };
    let v: Vec<&str> = b.split(',').collect();
    if v.len() < 3 { return None; }
    Some((ch(v[0])?, ch(v[1])?, ch(v[2])?))
}

fn named(s: &str) -> Option<(u8,u8,u8)> { Some(match s {
    "black"=>(0,0,0), "white"=>(255,255,255), "red"=>(255,0,0), "green"=>(0,128,0),
    "blue"=>(0,0,255), "yellow"=>(255,255,0), "orange"=>(255,165,0), "purple"=>(128,0,128),
    "pink"=>(255,192,203), "gray"|"grey"=>(128,128,128), "darkgray"|"darkgrey"=>(169,169,169),
    "lightgray"|"lightgrey"=>(211,211,211), "silver"=>(192,192,192),
    "cyan"|"aqua"=>(0,255,255), "magenta"|"fuchsia"=>(255,0,255),
    "lime"=>(0,255,0), "maroon"=>(128,0,0), "navy"=>(0,0,128),
    "olive"=>(128,128,0), "teal"=>(0,128,128), "transparent"=>(0,0,0),
    _=>return None,
})}

// ── transform parsing ─────────────────────────────────────────────────────────

fn parse_transform(s: &str) -> Option<Transform> {
    let mut result = Transform::identity();
    let mut rest   = s.trim();
    while !rest.is_empty() {
        rest = rest.trim_start();
        let p = rest.find('(')?;
        let name = rest[..p].trim();
        let after = &rest[p+1..];
        let q = after.find(')')?;
        let args: Vec<f32> = after[..q].split(|c: char| c==','||c.is_whitespace())
            .filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
        rest = after[q+1..].trim_start_matches(',').trim();
        let t = match name {
            "translate" => Transform::from_translate(*args.first().unwrap_or(&0.0), *args.get(1).unwrap_or(&0.0)),
            "scale"     => Transform::from_scale(*args.first().unwrap_or(&1.0), *args.get(1).unwrap_or(args.first().unwrap_or(&1.0))),
            "rotate"    => {
                let (deg,cx,cy) = (args.first().copied().unwrap_or(0.0), args.get(1).copied().unwrap_or(0.0), args.get(2).copied().unwrap_or(0.0));
                Transform::from_translate(cx,cy).post_concat(Transform::from_rotate(deg)).post_concat(Transform::from_translate(-cx,-cy))
            }
            "skewX" => Transform::from_skew(args.first().copied().unwrap_or(0.0).to_radians().tan(), 0.0),
            "skewY" => Transform::from_skew(0.0, args.first().copied().unwrap_or(0.0).to_radians().tan()),
            "matrix" if args.len() >= 6 => Transform::from_row(args[0],args[1],args[2],args[3],args[4],args[5]),
            _ => Transform::identity(),
        };
        result = result.post_concat(t);
    }
    Some(result)
}

// ── path data parser ──────────────────────────────────────────────────────────

fn parse_path_d(d: &str) -> Option<Path> {
    let toks = tokenize_path(d);
    let mut pb = PathBuilder::new();
    let mut i = 0usize;
    let mut cx = 0.0f32; let mut cy = 0.0f32;
    let mut sx = 0.0f32; let mut sy = 0.0f32; // last cubic ctrl
    let mut qx = 0.0f32; let mut qy = 0.0f32; // last quad ctrl
    let mut ox = 0.0f32; let mut oy = 0.0f32; // subpath origin
    let mut cmd = 'M';

    macro_rules! nf { () => {{ let v = toks.get(i).and_then(|t| t.parse::<f32>().ok()).unwrap_or(0.0); i+=1; v }} }

    while i < toks.len() {
        // A token is a command if it's a single ASCII letter
        if let Some(tok) = toks.get(i) {
            if tok.len() == 1 {
                if let Some(c) = tok.chars().next() {
                    if c.is_ascii_alphabetic() { cmd = c; i += 1; continue; }
                }
            }
        }
        // Reset smooth-curve reflections on non-curve commands
        if !"CcSsQqTt".contains(cmd) { sx=cx; sy=cy; qx=cx; qy=cy; }

        match cmd {
            'M' => { cx=nf!(); cy=nf!(); ox=cx; oy=cy; pb.move_to(cx,cy); cmd='L'; }
            'm' => { cx+=nf!(); cy+=nf!(); ox=cx; oy=cy; pb.move_to(cx,cy); cmd='l'; }
            'Z'|'z' => { pb.close(); cx=ox; cy=oy; }
            'L' => { cx=nf!(); cy=nf!(); pb.line_to(cx,cy); }
            'l' => { cx+=nf!(); cy+=nf!(); pb.line_to(cx,cy); }
            'H' => { cx=nf!(); pb.line_to(cx,cy); }
            'h' => { cx+=nf!(); pb.line_to(cx,cy); }
            'V' => { cy=nf!(); pb.line_to(cx,cy); }
            'v' => { cy+=nf!(); pb.line_to(cx,cy); }
            'C' => { let (x1,y1,x2,y2,x,y)=(nf!(),nf!(),nf!(),nf!(),nf!(),nf!()); pb.cubic_to(x1,y1,x2,y2,x,y); sx=x2;sy=y2;cx=x;cy=y; }
            'c' => { let (dx1,dy1,dx2,dy2,dx,dy)=(nf!(),nf!(),nf!(),nf!(),nf!(),nf!()); let(x1,y1,x2,y2,x,y)=(cx+dx1,cy+dy1,cx+dx2,cy+dy2,cx+dx,cy+dy); pb.cubic_to(x1,y1,x2,y2,x,y); sx=x2;sy=y2;cx=x;cy=y; }
            'S' => { let (x2,y2,x,y)=(nf!(),nf!(),nf!(),nf!()); pb.cubic_to(2.0*cx-sx,2.0*cy-sy,x2,y2,x,y); sx=x2;sy=y2;cx=x;cy=y; }
            's' => { let (dx2,dy2,dx,dy)=(nf!(),nf!(),nf!(),nf!()); let(x2,y2,x,y)=(cx+dx2,cy+dy2,cx+dx,cy+dy); pb.cubic_to(2.0*cx-sx,2.0*cy-sy,x2,y2,x,y); sx=x2;sy=y2;cx=x;cy=y; }
            'Q' => { let (x1,y1,x,y)=(nf!(),nf!(),nf!(),nf!()); pb.quad_to(x1,y1,x,y); qx=x1;qy=y1;cx=x;cy=y; }
            'q' => { let (dx1,dy1,dx,dy)=(nf!(),nf!(),nf!(),nf!()); let(x1,y1,x,y)=(cx+dx1,cy+dy1,cx+dx,cy+dy); pb.quad_to(x1,y1,x,y); qx=x1;qy=y1;cx=x;cy=y; }
            'T' => { let (x,y)=(nf!(),nf!()); pb.quad_to(2.0*cx-qx,2.0*cy-qy,x,y); qx=2.0*cx-qx;qy=2.0*cy-qy;cx=x;cy=y; }
            't' => { let (dx,dy)=(nf!(),nf!()); let(x,y)=(cx+dx,cy+dy); pb.quad_to(2.0*cx-qx,2.0*cy-qy,x,y); qx=2.0*cx-qx;qy=2.0*cy-qy;cx=x;cy=y; }
            'A' => { let (rx,ry,_,l,sw,x,y)=(nf!(),nf!(),nf!(),nf!()!=0.0,nf!()!=0.0,nf!(),nf!()); arc_to(&mut pb,cx,cy,rx,ry,l,sw,x,y); cx=x;cy=y; }
            'a' => { let (rx,ry,_,l,sw,dx,dy)=(nf!(),nf!(),nf!(),nf!()!=0.0,nf!()!=0.0,nf!(),nf!()); let(x,y)=(cx+dx,cy+dy); arc_to(&mut pb,cx,cy,rx,ry,l,sw,x,y); cx=x;cy=y; }
            _ => { i += 1; }
        }
    }
    pb.finish()
}

/// Tokenize SVG path data into command letters and number strings.
fn tokenize_path(d: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut num = String::new();
    let flush = |num: &mut String, out: &mut Vec<String>| {
        let s = num.trim().to_string();
        if !s.is_empty() { out.push(s); }
        num.clear();
    };
    let mut chars = d.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'A'..='Z' | 'a'..='z' => {
                // 'e'/'E' inside a number is scientific notation, not a command
                if (c == 'e' || c == 'E') && !num.is_empty() && num.chars().last().map_or(false, |x| x.is_ascii_digit() || x == '.') {
                    num.push(c);
                } else {
                    flush(&mut num, &mut out);
                    out.push(c.to_string());
                }
            }
            '0'..='9' | '.' => { num.push(c); }
            '-' => {
                if num.ends_with('e') || num.ends_with('E') { num.push(c); }
                else { flush(&mut num, &mut out); num.push(c); }
            }
            ' ' | '\t' | '\n' | '\r' | ',' => { flush(&mut num, &mut out); }
            _ => {}
        }
        let _ = chars.peek(); // keep peekable happy
    }
    flush(&mut num, &mut out);
    out
}

// ── arc → cubic Bezier ────────────────────────────────────────────────────────

fn arc_to(pb: &mut PathBuilder, x1: f32, y1: f32, mut rx: f32, mut ry: f32,
          large: bool, sweep: bool, x2: f32, y2: f32) {
    if rx == 0.0 || ry == 0.0 { pb.line_to(x2, y2); return; }
    rx = rx.abs(); ry = ry.abs();
    let (dx, dy) = ((x1-x2)/2.0, (y1-y2)/2.0);
    let lam = dx*dx/(rx*rx) + dy*dy/(ry*ry);
    if lam > 1.0 { let s=lam.sqrt(); rx*=s; ry*=s; }
    let sign = if large==sweep { -1.0f32 } else { 1.0 };
    let num = (rx*rx*ry*ry - rx*rx*dy*dy - ry*ry*dx*dx).max(0.0);
    let den = rx*rx*dy*dy + ry*ry*dx*dx;
    let k   = if den==0.0 { 0.0 } else { sign*(num/den).sqrt() };
    let (cx1, cy1) = (k*rx*dy/ry, k*(-ry)*dx/rx);
    let (cx,  cy)  = (cx1+(x1+x2)/2.0, cy1+(y1+y2)/2.0);
    let theta1  = ang(1.0,0.0,(dx-cx1)/rx,(dy-cy1)/ry);
    let mut dth = ang((dx-cx1)/rx,(dy-cy1)/ry,(-dx-cx1)/rx,(-dy-cy1)/ry)
                  % (2.0*std::f32::consts::PI);
    if !sweep && dth > 0.0 { dth -= 2.0*std::f32::consts::PI; }
    if  sweep && dth < 0.0 { dth += 2.0*std::f32::consts::PI; }
    let n = ((dth.abs()/(std::f32::consts::PI/2.0)).ceil() as usize).max(1);
    let dt = dth / n as f32;
    let mut t = theta1;
    for _ in 0..n { arc_seg(pb,cx,cy,rx,ry,t,t+dt); t+=dt; }
}

fn ang(ux:f32,uy:f32,vx:f32,vy:f32)->f32 { (ux*vy-uy*vx).atan2(ux*vx+uy*vy) }

fn arc_seg(pb: &mut PathBuilder, cx:f32,cy:f32,rx:f32,ry:f32,t1:f32,t2:f32) {
    let a  = (t2-t1).sin()*((4.0-(t2-t1).cos())/3.0)/(1.0+(t2-t1).cos());
    let (c1,s1)=(t1.cos(),t1.sin()); let (c2,s2)=(t2.cos(),t2.sin());
    let (p1x,p1y)=(cx+rx*c1,cy+ry*s1); let (p2x,p2y)=(cx+rx*c2,cy+ry*s2);
    pb.cubic_to(p1x-rx*s1*a, p1y+ry*c1*a, p2x+rx*s2*a, p2y-ry*c2*a, p2x,p2y);
}

// ── geometry helpers ──────────────────────────────────────────────────────────

fn ellipse_path(cx:f32,cy:f32,rx:f32,ry:f32)->Path {
    const K:f32=0.5522847;
    let (kx,ky)=(rx*K,ry*K);
    let mut pb=PathBuilder::new();
    pb.move_to(cx+rx,cy);
    pb.cubic_to(cx+rx,cy-ky, cx+kx,cy-ry, cx,   cy-ry);
    pb.cubic_to(cx-kx,cy-ry, cx-rx,cy-ky, cx-rx,cy   );
    pb.cubic_to(cx-rx,cy+ky, cx-kx,cy+ry, cx,   cy+ry);
    pb.cubic_to(cx+kx,cy+ry, cx+rx,cy+ky, cx+rx,cy   );
    pb.close();
    pb.finish().unwrap_or_else(||PathBuilder::new().finish().unwrap())
}

fn rounded_rect(x:f32,y:f32,w:f32,h:f32,rx:f32,ry:f32)->Path {
    const K:f32=0.5522847;
    let (kx,ky)=(rx*K,ry*K); let (r,b)=(x+w,y+h);
    let mut pb=PathBuilder::new();
    pb.move_to(x+rx,y); pb.line_to(r-rx,y);
    pb.cubic_to(r-rx+kx,y,  r,y+ry-ky,  r,y+ry); pb.line_to(r,b-ry);
    pb.cubic_to(r,b-ry+ky,  r-rx+kx,b,  r-rx,b); pb.line_to(x+rx,b);
    pb.cubic_to(x+rx-kx,b,  x,b-ry+ky,  x,b-ry); pb.line_to(x,y+ry);
    pb.cubic_to(x,y+ry-ky,  x+rx-kx,y,  x+rx,y); pb.close();
    pb.finish().unwrap_or_else(||PathBuilder::new().finish().unwrap())
}

// ── SVG attribute helpers ─────────────────────────────────────────────────────

fn parse_viewbox(el: &El) -> (f32,f32,f32,f32) {
    el.attr("viewBox").and_then(|s| {
        let v: Vec<f32> = s.split(|c:char| c==','||c.is_whitespace())
            .filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
        if v.len()>=4 { Some((v[0],v[1],v[2],v[3])) } else { None }
    }).unwrap_or((0.0,0.0,0.0,0.0))
}

fn attr_f32(el: &El, attr: &str, fallback: f32) -> f32 {
    el.attr(attr).map(|s| parse_length(s, fallback)).unwrap_or(fallback)
}

fn parse_length(s: &str, vp: f32) -> f32 {
    let s = s.trim();
    if s.ends_with('%') { s.trim_end_matches('%').parse::<f32>().unwrap_or(100.0)/100.0*vp }
    else { s.chars().take_while(|c| c.is_ascii_digit()||*c=='.'||*c=='-').collect::<String>().parse().unwrap_or(vp) }
}

fn af(el: &El, attr: &str) -> f32 { parse_f32(el.attr(attr).unwrap_or("0")) }
fn parse_f32(s: &str) -> f32 { s.trim().trim_end_matches("px").parse().unwrap_or(0.0) }

fn is_hidden(el: &El) -> bool {
    let s = el.attr("style").unwrap_or("");
    style_prop(s,"display")==Some("none") || style_prop(s,"visibility")==Some("hidden")
    || el.attr("display")==Some("none") || el.attr("visibility")==Some("hidden")
}
