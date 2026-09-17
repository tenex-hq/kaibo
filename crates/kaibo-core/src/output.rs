//! Output contract.
//!
//! A verb returns a typed value implementing [`Render`], and the binary
//! chooses text or JSON at the last moment - a verb never branches on the
//! output flag itself.
//!
//! Default output is compact, human-readable text, on a TTY and off it
//! alike; `--json` is opt-in. This is deliberate and is not the usual "JSON
//! when not a TTY" convention: the primary consumer is a model reading
//! stdout, and JSON costs materially more tokens for the same fields.

use serde_json::Value;

/// Rendering options threaded from global CLI flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    /// `--full`: show wide output. Default schemas are narrow (3-4 fields
    /// per list item); this is the escape hatch.
    pub full: bool,
}

/// A value a verb hands back to the binary, renderable as either compact
/// text or JSON.
pub trait Render {
    /// Compact, human (and model) readable text. Default schemas are
    /// narrow; widen only when `options.full` is set.
    fn render_text(&self, options: &RenderOptions) -> String;

    /// The same information as `render_text`, as JSON. JSON consumers get
    /// the full shape regardless of `--full`, since the narrow/full
    /// distinction exists only to save tokens in text output.
    fn render_json(&self) -> Value;
}
