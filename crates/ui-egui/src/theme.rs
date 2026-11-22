//! Theme tokens: every colour and size the interface draws with (AGENTS.md rule 14). Never
//! hard-code a colour in a widget; add a token. Text tokens keep 4.5:1 contrast against the
//! surfaces they are drawn on, and control outlines 3:1, in both themes (standards/accessibility.md);
//! `tests/it/contrast.rs` checks the pairs.

use egui::{Color32, FontFamily, FontId, Stroke, Visuals};
use texcraft_latex::Kind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub scheme: Scheme,
    /// The window behind every panel.
    pub background: Color32,
    /// Side panel, toolbars, the rail.
    pub panel: Color32,
    /// The editor and other text fields.
    pub input: Color32,
    /// Behind the PDF pages.
    pub canvas: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    /// Text drawn on `accent`.
    pub on_accent: Color32,
    pub selection: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub info: Color32,
    /// Behind a source line with an error.
    pub error_line: Color32,
    pub syntax_command: Color32,
    pub syntax_comment: Color32,
    pub syntax_group: Color32,
    pub syntax_math: Color32,
    pub syntax_environment: Color32,
    pub syntax_special: Color32,
    pub font_size: f32,
    pub code_size: f32,
    pub radius: u8,
    pub rail_width: f32,
    pub gap: f32,
}

impl Tokens {
    pub fn dark() -> Self {
        Self {
            scheme: Scheme::Dark,
            background: Color32::from_rgb(0x1b, 0x1d, 0x21),
            panel: Color32::from_rgb(0x24, 0x27, 0x2c),
            input: Color32::from_rgb(0x1e, 0x20, 0x24),
            canvas: Color32::from_rgb(0x3a, 0x3e, 0x45),
            border: Color32::from_rgb(0x6b, 0x71, 0x7c),
            text: Color32::from_rgb(0xe8, 0xea, 0xed),
            text_dim: Color32::from_rgb(0xa9, 0xaf, 0xb8),
            accent: Color32::from_rgb(0x8a, 0xb4, 0xf8),
            on_accent: Color32::from_rgb(0x10, 0x1a, 0x2e),
            selection: Color32::from_rgba_unmultiplied(0x8a, 0xb4, 0xf8, 0x50),
            error: Color32::from_rgb(0xff, 0x8a, 0x80),
            warning: Color32::from_rgb(0xf6, 0xc3, 0x5b),
            info: Color32::from_rgb(0x8a, 0xc8, 0xf0),
            error_line: Color32::from_rgba_unmultiplied(0xff, 0x6b, 0x6b, 0x28),
            syntax_command: Color32::from_rgb(0x8a, 0xb4, 0xf8),
            syntax_comment: Color32::from_rgb(0x9a, 0xa3, 0x8f),
            syntax_group: Color32::from_rgb(0xc8, 0xb0, 0xe8),
            syntax_math: Color32::from_rgb(0x7f, 0xd6, 0xb0),
            syntax_environment: Color32::from_rgb(0xf0, 0xb4, 0x7a),
            syntax_special: Color32::from_rgb(0xf6, 0xc3, 0x5b),
            font_size: 14.0,
            code_size: 14.0,
            radius: 4,
            rail_width: 44.0,
            gap: 6.0,
        }
    }

    pub fn light() -> Self {
        Self {
            scheme: Scheme::Light,
            background: Color32::from_rgb(0xf3, 0xf4, 0xf6),
            panel: Color32::from_rgb(0xfa, 0xfa, 0xfb),
            input: Color32::WHITE,
            canvas: Color32::from_rgb(0xd5, 0xd8, 0xdd),
            border: Color32::from_rgb(0x86, 0x8c, 0x96),
            text: Color32::from_rgb(0x1d, 0x1f, 0x23),
            text_dim: Color32::from_rgb(0x52, 0x58, 0x61),
            accent: Color32::from_rgb(0x1f, 0x56, 0xb8),
            on_accent: Color32::WHITE,
            selection: Color32::from_rgba_unmultiplied(0x1f, 0x56, 0xb8, 0x40),
            error: Color32::from_rgb(0xb3, 0x26, 0x1e),
            warning: Color32::from_rgb(0x8a, 0x5a, 0x00),
            info: Color32::from_rgb(0x1f, 0x56, 0xb8),
            error_line: Color32::from_rgba_unmultiplied(0xd9, 0x30, 0x25, 0x1f),
            syntax_command: Color32::from_rgb(0x1f, 0x56, 0xb8),
            syntax_comment: Color32::from_rgb(0x4f, 0x6b, 0x3a),
            syntax_group: Color32::from_rgb(0x6f, 0x3f, 0xa8),
            syntax_math: Color32::from_rgb(0x0b, 0x6e, 0x4f),
            syntax_environment: Color32::from_rgb(0x9a, 0x45, 0x00),
            syntax_special: Color32::from_rgb(0x8a, 0x5a, 0x00),
            ..Self::dark()
        }
    }

    pub fn for_scheme(scheme: Scheme) -> Self {
        match scheme {
            Scheme::Dark => Self::dark(),
            Scheme::Light => Self::light(),
        }
    }

    pub fn syntax(&self, kind: Kind) -> Color32 {
        match kind {
            Kind::Text => self.text,
            Kind::Command => self.syntax_command,
            Kind::Comment => self.syntax_comment,
            Kind::Group => self.syntax_group,
            Kind::Special => self.syntax_special,
            Kind::MathDelimiter | Kind::Math => self.syntax_math,
            Kind::Environment => self.syntax_environment,
        }
    }

    pub fn code_font(&self) -> FontId {
        FontId::new(self.code_size, FontFamily::Monospace)
    }

    /// Applies the tokens to egui's own widgets.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = match self.scheme {
            Scheme::Dark => Visuals::dark(),
            Scheme::Light => Visuals::light(),
        };
        visuals.panel_fill = self.panel;
        visuals.window_fill = self.panel;
        visuals.extreme_bg_color = self.input;
        visuals.faint_bg_color = self.background;
        visuals.override_text_color = Some(self.text);
        visuals.hyperlink_color = self.accent;
        visuals.selection.bg_fill = self.selection;
        visuals.selection.stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.error_fg_color = self.error;
        visuals.warn_fg_color = self.warning;
        ctx.set_visuals(visuals);
        ctx.global_style_mut(|style| {
            for (text_style, font) in &mut style.text_styles {
                if *text_style != egui::TextStyle::Heading {
                    font.size = self.font_size;
                }
            }
            style.spacing.item_spacing = egui::vec2(self.gap, self.gap * 0.75);
        });
    }
}

/// WCAG 2.2 contrast ratio between two opaque colours.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn luminance(c: Color32) -> f32 {
        let channel = |v: u8| {
            let s = f32::from(v) / 255.0;
            if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}
