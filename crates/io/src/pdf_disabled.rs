//! Selected profile: PDF output is explicitly unavailable; shared plot geometry remains upstream.
use crate::{IoError, Result};
use cadcraft_doc::{Drawing, Handle, PageSetup, Space};
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::{Sheet, paper};
use serde_json::Value;
const MAX_SHEET_MM: f64 = 5080.0;
/// Most sheets (pages) in one file.
pub const MAX_PAGES: usize = 256;

const MAX_LINKS: usize = 10_000;

/// model space).
#[derive(Clone, Debug)]
pub struct PdfOptions {
    /// Plot style table name, replacing the page setup's (`"None"` plots without one).
    pub plot_style_table: Option<String>,
    /// Paper name from [`cadcraft_render::PAPER_SIZES`] (`"A4"`, `"Letter"`, `"ANSI B"`…).
    pub paper: Option<String>,
    /// Custom paper size in millimetres (portrait width, height).
    pub paper_mm: Option<(f64, f64)>,
    pub landscape: Option<bool>,
    /// Fit the drawing to the printable area. Default: on for model space, off for layouts (1:1).
    pub fit: Option<bool>,
    /// Plot scale in paper units per drawing unit (when not fitting).
    pub scale: Option<f64>,
    /// Model-space plot placement. `None` keeps the legacy centered behavior.
    pub center: Option<bool>,
    /// Model-space X/Y plot offset in millimetres from the printable area's lower-left corner.
    pub offset: Option<[f64; 2]>,
    /// Plot object lineweights. Default: the layout's setting; on for model space.
    pub lineweights: Option<bool>,
    /// Flate-compress the content stream.
    pub compress: bool,
    pub title: String,
    /// Model space only: plot this rectangle (drawing units), fitted to the printable area,
    /// instead of the extents. Ignored for layouts.
    pub window: Option<Bounds2>,
    /// Drawing layers become PDF layers (optional content groups). Default on.
    pub layers: bool,
    /// With `layers`: objects on layers that are off are plotted on PDF layers that start
    /// hidden (frozen layers are never plotted). Default off: they aren't plotted.
    pub include_off_layers: bool,
    /// Hyperlinks: an entity's handle and its link target (a URL). The area the entity covers
    /// on the sheet becomes a link annotation.
    pub links: Vec<(Handle, String)>,
    /// Write text in installed TrueType fonts as filled outlines instead of as text in an
    /// embedded font subset (like AutoCAD's PDFSHX for its own fonts). Stroke-font text is
    /// always geometry.
    pub text_as_geometry: bool,
    /// Margins (left, bottom, right, top; mm) of the plot device's paper, bounding the printable
    /// area of a model-space plot. `None` keeps the page setup's (none for model space).
    pub margins: Option<[f64; 4]>,
    /// Entity hyperlinks read from a drawing file (`PE_URL` extended data) become link
    /// annotations too, after [`PdfOptions::links`]. Default on.
    pub hyperlinks: bool,
    /// Plot objects with their transparency (PDF alpha). `None` = the page setup's "Plot
    /// transparency" (off for model space).
    pub plot_transparency: Option<bool>,
}

impl Default for PdfOptions {
    fn default() -> Self {
        PdfOptions {
            plot_style_table: None,
            paper: None,
            paper_mm: None,
            landscape: None,
            fit: None,
            scale: None,
            center: None,
            offset: None,
            lineweights: None,
            compress: false,
            title: String::new(),
            window: None,
            layers: true,
            include_off_layers: false,
            links: Vec::new(),
            text_as_geometry: false,
            margins: None,
            hyperlinks: true,
            plot_transparency: None,
        }
    }
}

impl PdfOptions {
    /// Parse `{paper?, width?, height?, landscape?, fit?, scale?, center?, offset?: [x,y] mm,
    /// lineweights?, compress?, title?, window?: [[x1, y1], [x2, y2]], margins?: [l, b, r, t] (mm),
    /// hyperlinks?: bool, plotTransparency?: bool}`.
    pub fn from_json(v: &Value) -> PdfOptions {
        let num = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        let paper_mm = match (num("width"), num("height")) {
            (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w, h)),
            _ => None,
        };
        PdfOptions {
            // `layers?` (default on), `offLayers?`, `links?: [{handle, url}]`, `text?: "text" |
            // "geometry"`.
            text_as_geometry: v.get("text").and_then(Value::as_str).is_some_and(|t| t.eq_ignore_ascii_case("geometry")),
            layers: v.get("layers").and_then(Value::as_bool).unwrap_or(true),
            include_off_layers: v.get("offLayers").and_then(Value::as_bool).unwrap_or(false),
            links: links_json(v.get("links")),
            plot_style_table: v.get("plotStyleTable").and_then(Value::as_str).map(|s| s.chars().take(260).collect()),
            paper: v.get("paper").and_then(Value::as_str).map(str::to_string),
            paper_mm,
            landscape: v.get("landscape").and_then(Value::as_bool),
            fit: v.get("fit").and_then(Value::as_bool),
            scale: num("scale").filter(|s| *s > 0.0),
            center: v.get("center").and_then(Value::as_bool),
            offset: offset(v.get("offset")),
            lineweights: v.get("lineweights").and_then(Value::as_bool),
            compress: v.get("compress").and_then(Value::as_bool).unwrap_or(true),
            title: v.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
            window: window(v.get("window")),
            margins: margins(v.get("margins")),
            hyperlinks: v.get("hyperlinks").and_then(Value::as_bool).unwrap_or(true),
            plot_transparency: v.get("plotTransparency").and_then(Value::as_bool),
        }
    }
}

fn offset(v: Option<&Value>) -> Option<[f64; 2]> {
    let a = v?.as_array()?;
    if a.len() != 2 {
        return None;
    }
    let x = a.first()?.as_f64()?;
    let y = a.get(1)?.as_f64()?;
    valid_plot_offset([x, y]).then_some([x, y])
}

fn valid_plot_offset(offset: [f64; 2]) -> bool {
    offset.iter().all(|v| v.is_finite() && v.abs() <= paper::MAX_PLOT_OFFSET_MM)
}

/// Reject malformed offsets at the JSON plotting boundary, before a permissive option parser
/// could treat them as absent.
pub(crate) fn validate_plot_offset_json(v: &Value) -> Result<()> {
    if let Some(raw) = v.get("offset")
        && offset(Some(raw)).is_none()
    {
        return Err(IoError::Format(format!(
            "plot offset must contain exactly two finite values from -{} to {} mm",
            paper::MAX_PLOT_OFFSET_MM,
            paper::MAX_PLOT_OFFSET_MM
        )));
    }
    Ok(())
}

/// `[l, b, r, t]` margins in mm (finite, 0–1000), else `None`.
pub(crate) fn margins(v: Option<&Value>) -> Option<[f64; 4]> {
    let a: Vec<f64> = v?.as_array()?.iter().take(5).filter_map(Value::as_f64).collect();
    match a[..] {
        [l, b, r, t] if a.iter().all(|x| x.is_finite() && (0.0..=1000.0).contains(x)) => Some([l, b, r, t]),
        _ => None,
    }
}

/// A plot window from `[[x1, y1], [x2, y2]]`: finite corners and a positive size, else `None`.
fn window(v: Option<&Value>) -> Option<Bounds2> {
    let corner = |i: usize| {
        let c = v?.get(i)?;
        let p = Vec2::new(c.get(0)?.as_f64()?, c.get(1)?.as_f64()?);
        p.is_finite().then_some(p)
    };
    let (a, b) = (corner(0)?, corner(1)?);
    let w = Bounds2::new(a, b);
    (w.width() > 0.0 && w.height() > 0.0).then_some(w)
}

fn links_json(v: Option<&Value>) -> Vec<(Handle, String)> {
    let Some(a) = v.and_then(Value::as_array) else { return Vec::new() };
    a.iter()
        .take(MAX_LINKS)
        .filter_map(|l| {
            let h = l.get("handle")?;
            let h = h.as_str().and_then(Handle::parse_hex).or_else(|| h.as_u64().map(Handle))?;
            let url = l.get("url")?.as_str()?.trim();
            (!url.is_empty()).then(|| (h, url.to_string()))
        })
        .collect()
}

/// The page setup a plot of `space` uses, with overrides applied.
pub fn page_for(d: &Drawing, space: &Space, o: &PdfOptions) -> Result<PageSetup> {
    let mut page = match space.layout() {
        Some(n) => d.layout(n).map(|l| l.page.clone()).ok_or_else(|| IoError::Format(format!("no layout `{n}`")))?,
        None => {
            let mut p = PageSetup::default();
            if paper::paper_unit_mm(d) == 1.0
                && let Some(a4) = paper::paper_size("A4")
            {
                p.paper = a4.name.into();
                p.width_mm = a4.width_mm;
                p.height_mm = a4.height_mm;
            }
            p
        }
    };
    if let Some(name) = &o.paper {
        let ps = paper::paper_size(name).ok_or_else(|| IoError::Format(format!("unknown paper size `{name}`")))?;
        page.paper = ps.name.into();
        page.width_mm = ps.width_mm;
        page.height_mm = ps.height_mm;
    }
    if let Some((w, h)) = o.paper_mm {
        page.paper = format!("User ({w:.2} x {h:.2} MM)");
        page.width_mm = w.min(MAX_SHEET_MM);
        page.height_mm = h.min(MAX_SHEET_MM);
    }
    if let Some(l) = o.landscape {
        page.landscape = l;
    }
    if let Some(t) = &o.plot_style_table {
        page.plot_style_table = t.clone();
    }
    // The plot device's margins bound a model-space plot (a layout's come with its page setup).
    if let (Space::Model, Some(m)) = (space, o.margins) {
        page.margins_mm = m;
    }
    page.width_mm = page.width_mm.clamp(1.0, MAX_SHEET_MM);
    page.height_mm = page.height_mm.clamp(1.0, MAX_SHEET_MM);
    Ok(page)
}

pub(crate) fn with_saved_setup(d: &Drawing, space: &Space, o: &PdfOptions) -> PdfOptions {
    let mut o = o.clone();
    let Some(page) = space.layout().and_then(|n| d.layout(n)).map(|l| &l.page) else { return o };
    // An explicit `fit: false` without a `scale` asks for the plain 1:1 plot of the sheet.
    if o.fit == Some(false) && o.scale.is_none() {
        o.window = None;
        return o;
    }
    let saved = paper::saved_plot(page);
    // A layout is plotted over its own sheet: only its saved window applies, not one in the options.
    o.window = saved.window;
    // An explicit scale beats a saved scale to fit (the scale is ignored while fitting).
    if o.fit.is_none() && o.scale.is_none() && saved.fit {
        o.fit = Some(true);
    }
    if o.scale.is_none() {
        o.scale = saved.scale;
    }
    if o.center.is_none() && saved.window.is_some() {
        o.center = Some(saved.center);
    }
    o
}

pub(crate) fn placement(space: &Space, bounds: &Bounds2, fit: bool, sheet: &Sheet, o: &PdfOptions) -> (Vec2, Vec2) {
    if matches!(space, Space::Model) {
        let from = if bounds.is_empty() {
            Vec2::ZERO
        } else if o.center.unwrap_or(true) {
            bounds.center()
        } else {
            bounds.min
        };
        let to = if o.center.unwrap_or(true) {
            sheet.printable.center()
        } else {
            let offset = o.offset.unwrap_or([0.0, 0.0]);
            sheet.printable.min + Vec2::new(offset[0] / sheet.unit_mm, offset[1] / sheet.unit_mm)
        };
        (from, to)
    } else if fit {
        (if bounds.is_empty() { Vec2::ZERO } else { bounds.center() }, sheet.printable.center())
    } else {
        (Vec2::ZERO, Vec2::ZERO)
    }
}

pub(crate) fn plot_scale(b: &Bounds2, sheet: &Sheet, fit: bool, scale: Option<f64>) -> f64 {
    if let Some(s) = scale.filter(|s| s.is_finite() && *s > 0.0)
        && !fit
    {
        return s;
    }
    if !fit || b.is_empty() {
        return 1.0;
    }
    let pw = sheet.printable.width();
    let ph = sheet.printable.height();
    let s = (pw / b.width().max(1e-12)).min(ph / b.height().max(1e-12));
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

pub fn plot(_: &Drawing, _: &Space, _: &Value) -> Result<Vec<u8>> {
    Err(IoError::Unsupported("PDF disabled in selected Slab profile".into()))
}
pub fn pdf(_: &Drawing, _: &Space, _: &PdfOptions) -> Result<Vec<u8>> {
    Err(IoError::Unsupported("PDF disabled in selected Slab profile".into()))
}
