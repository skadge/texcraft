//! Theme tokens: every colour and size the interface draws with (AGENTS.md rule 14; values and
//! rationale in docs/design.md). Never hard-code a colour or a size in a widget; add a token. Text
//! tokens keep 4.5:1 contrast against the surfaces they are drawn on, control outlines and the
//! accent 3:1, in both themes (standards/accessibility.md); `tests/it/contrast.rs` checks them.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Visuals};
use texcraft_latex::Kind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub scheme: Scheme,
    // Surfaces
    /// Top bar, rail, side panel, toolbars.
    pub frame: Color32,
    /// Editor, dialogs, cards.
    pub surface: Color32,
    /// Behind the PDF pages.
    pub canvas: Color32,
    /// Menus, popovers, tooltips.
    pub raised: Color32,
    /// Hairlines between regions; decorative.
    pub separator: Color32,
    /// Outlines of fields and buttons.
    pub control_border: Color32,
    // Text and meaning
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    /// Selected rows and active toggles.
    pub accent_soft: Color32,
    /// Selected text.
    pub selection: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub success: Color32,
    pub note: Color32,
    /// The wavy line under a misspelled word, set apart from the problems' colours.
    pub spelling: Color32,
    /// Behind a source line with an error.
    pub error_line: Color32,
    /// Behind the line with the caret.
    pub current_line: Color32,
    /// Behind search matches in the source.
    pub search_match: Color32,
    /// Behind a line a version added, and one it removed, in a comparison of two versions.
    pub diff_added: Color32,
    pub diff_removed: Color32,
    /// Vim's block caret; the character under it is drawn in `caret_text`.
    pub caret_block: Color32,
    pub caret_text: Color32,
    pub shadow: Color32,
    /// Behind the page in presentation mode.
    pub presentation: Color32,
    // Syntax
    pub syntax_command: Color32,
    pub syntax_comment: Color32,
    pub syntax_group: Color32,
    pub syntax_math: Color32,
    pub syntax_environment: Color32,
    pub syntax_special: Color32,
    // Type
    pub font_size: f32,
    pub small_size: f32,
    pub title_size: f32,
    pub code_size: f32,
    pub code_line_height: f32,
    /// The formatted view's text (docs/design.md, Type).
    pub reading_size: f32,
    /// Its line height, as a multiple of the size.
    pub reading_leading: f32,
    /// Its measure, in average characters.
    pub reading_measure: f32,
    /// Its headings, from `\part` (0) to `\subparagraph` (6).
    pub heading_sizes: [f32; 7],
    // Shape
    pub radius: u8,
    pub radius_large: u8,
    // Fixed heights and widths (docs/design.md, Space and shape)
    pub top_bar_height: f32,
    pub toolbar_height: f32,
    pub tab_height: f32,
    pub status_height: f32,
    pub rail_width: f32,
    pub rail_button: f32,
    pub row_height: f32,
    pub icon_size: f32,
    pub rail_icon_size: f32,
    /// The grid unit; gaps are multiples of it.
    pub unit: f32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(c.r(), c.g(), c.b(), a)
}

impl Tokens {
    pub fn light() -> Self {
        let accent = rgb(0x2557D6);
        let text = rgb(0x1B1E23);
        Self {
            scheme: Scheme::Light,
            frame: rgb(0xF4F5F7),
            surface: rgb(0xFFFFFF),
            canvas: rgb(0xE6E8EC),
            raised: rgb(0xFFFFFF),
            separator: rgb(0xE1E4E8),
            control_border: rgb(0x727985),
            text,
            text_muted: rgb(0x5B626C),
            accent,
            on_accent: rgb(0xFFFFFF),
            accent_soft: alpha(accent, 26),
            selection: alpha(accent, 56),
            hover: alpha(text, 15),
            pressed: alpha(text, 26),
            error: rgb(0xB0261A),
            warning: rgb(0x7F5200),
            success: rgb(0x17683B),
            note: rgb(0x2557D6),
            spelling: rgb(0xA3249E),
            error_line: alpha(rgb(0xC42B1C), 22),
            current_line: alpha(rgb(0x1B1E23), 8),
            search_match: alpha(rgb(0xE8A317), 77),
            diff_added: alpha(rgb(0x17683B), 31),
            diff_removed: alpha(rgb(0xC42B1C), 26),
            caret_block: text,
            caret_text: rgb(0xFFFFFF),
            shadow: Color32::from_black_alpha(36),
            presentation: Color32::BLACK,
            syntax_command: rgb(0x2050C0),
            syntax_comment: rgb(0x5A6B4E),
            syntax_group: rgb(0x6A3FA0),
            syntax_math: rgb(0x0B6B4C),
            syntax_environment: rgb(0x9A4600),
            syntax_special: rgb(0x8A5A00),
            ..Self::dark()
        }
    }

    pub fn dark() -> Self {
        let accent = rgb(0x7AA7FF);
        let text = rgb(0xE7E9EC);
        Self {
            scheme: Scheme::Dark,
            frame: rgb(0x17191C),
            surface: rgb(0x1E2024),
            canvas: rgb(0x2A2D32),
            raised: rgb(0x25282D),
            separator: rgb(0x2C3036),
            control_border: rgb(0x7D8490),
            text,
            text_muted: rgb(0xA2A8B1),
            accent,
            on_accent: rgb(0x0D1830),
            accent_soft: alpha(accent, 41),
            selection: alpha(accent, 77),
            hover: alpha(text, 15),
            pressed: alpha(text, 26),
            error: rgb(0xFF8A80),
            warning: rgb(0xF2C45A),
            success: rgb(0x6FD39A),
            note: rgb(0x7AA7FF),
            spelling: rgb(0xF08BE4),
            error_line: alpha(rgb(0xFF6B6B), 36),
            current_line: alpha(rgb(0xFFFFFF), 10),
            search_match: alpha(rgb(0xF2C45A), 64),
            diff_added: alpha(rgb(0x6FD39A), 38),
            diff_removed: alpha(rgb(0xFF6B6B), 38),
            caret_block: text,
            caret_text: rgb(0x16181C),
            shadow: Color32::from_black_alpha(102),
            presentation: Color32::BLACK,
            syntax_command: rgb(0x82AEFF),
            syntax_comment: rgb(0x93A38D),
            syntax_group: rgb(0xC3A6F0),
            syntax_math: rgb(0x6FD3A8),
            syntax_environment: rgb(0xF0AE72),
            syntax_special: rgb(0xF2C45A),
            font_size: 13.0,
            small_size: 12.0,
            title_size: 16.0,
            code_size: 13.5,
            code_line_height: 21.0,
            reading_size: 16.0,
            reading_leading: 1.6,
            reading_measure: 70.0,
            heading_sizes: [30.0, 26.0, 22.0, 18.5, 16.5, 16.0, 16.0],
            radius: 6,
            radius_large: 8,
            top_bar_height: 44.0,
            toolbar_height: 36.0,
            tab_height: 32.0,
            status_height: 24.0,
            rail_width: 48.0,
            rail_button: 36.0,
            row_height: 26.0,
            icon_size: 16.0,
            rail_icon_size: 18.0,
            unit: 4.0,
        }
    }

    pub fn for_scheme(scheme: Scheme) -> Self {
        match scheme {
            Scheme::Dark => Self::dark(),
            Scheme::Light => Self::light(),
        }
    }

    /// The theme matching the system's, light when it is unknown.
    pub fn system(ctx: &egui::Context) -> Self {
        match ctx.system_theme() {
            Some(egui::Theme::Dark) => Self::dark(),
            _ => Self::light(),
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

    pub fn body(&self) -> FontId {
        FontId::proportional(self.font_size)
    }

    pub fn small(&self) -> FontId {
        FontId::proportional(self.small_size)
    }

    pub fn strong(&self) -> FontId {
        FontId::new(self.font_size, crate::fonts::semibold())
    }

    pub fn title(&self) -> FontId {
        FontId::new(self.title_size, crate::fonts::semibold())
    }

    /// The formatted view's text face at `size`, in its weight and slant.
    pub fn reading(&self, size: f32, bold: bool, italic: bool) -> FontId {
        let family = match (bold, italic) {
            (true, _) => crate::fonts::reading_semibold(),
            (false, true) => crate::fonts::reading_italic(),
            (false, false) => crate::fonts::reading(),
        };
        FontId::new(size, family)
    }

    pub fn corner(&self) -> CornerRadius {
        CornerRadius::same(self.radius)
    }

    pub fn corner_large(&self) -> CornerRadius {
        CornerRadius::same(self.radius_large)
    }

    pub fn hairline(&self) -> Stroke {
        Stroke::new(1.0, self.separator)
    }

    /// Applies the tokens to egui's own widgets, menus, tooltips and text styles.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = match self.scheme {
            Scheme::Dark => Visuals::dark(),
            Scheme::Light => Visuals::light(),
        };
        v.panel_fill = self.frame;
        v.window_fill = self.raised;
        v.window_stroke = self.hairline();
        v.window_corner_radius = self.corner_large();
        v.menu_corner_radius = self.corner_large();
        let shadow = Shadow { offset: [0, 6], blur: 20, spread: 0, color: self.shadow };
        v.window_shadow = shadow;
        v.popup_shadow = shadow;
        v.extreme_bg_color = self.surface;
        v.faint_bg_color = self.frame;
        v.code_bg_color = self.frame;
        v.override_text_color = Some(self.text);
        v.weak_text_color = Some(self.text_muted);
        v.hyperlink_color = self.accent;
        v.selection.bg_fill = self.selection;
        v.selection.stroke = Stroke::new(1.0, self.accent);
        v.error_fg_color = self.error;
        v.warn_fg_color = self.warning;
        v.text_cursor.stroke = Stroke::new(2.0, self.accent);

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.frame;
        w.noninteractive.weak_bg_fill = self.frame;
        w.noninteractive.bg_stroke = self.hairline();
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        // The fill of slider tracks and check boxes: a step off the surfaces they sit on.
        w.inactive.bg_fill = blend(self.surface, self.pressed);
        w.inactive.weak_bg_fill = self.surface;
        w.inactive.bg_stroke = Stroke::new(1.0, self.control_border);
        w.inactive.fg_stroke = Stroke::new(1.0, self.text);
        w.hovered.bg_fill = self.surface;
        w.hovered.weak_bg_fill = blend(self.surface, self.hover);
        w.hovered.bg_stroke = Stroke::new(1.0, self.text_muted);
        w.hovered.fg_stroke = Stroke::new(1.0, self.text);
        w.hovered.expansion = 0.0;
        w.active.bg_fill = self.surface;
        w.active.weak_bg_fill = blend(self.surface, self.pressed);
        w.active.bg_stroke = Stroke::new(1.0, self.accent);
        w.active.fg_stroke = Stroke::new(1.0, self.text);
        w.active.expansion = 0.0;
        w.open = w.active;
        for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            wv.corner_radius = self.corner();
        }
        // egui keeps visuals per theme and follows the system's by default: pinned to this
        // scheme, so that what egui draws itself (separators, menus) matches the tokens even when
        // the system's theme is not known yet (the browser's first frames) or differs.
        let theme = match self.scheme {
            Scheme::Dark => egui::Theme::Dark,
            Scheme::Light => egui::Theme::Light,
        };
        ctx.set_theme(theme);
        ctx.set_visuals_of(theme, v);

        let (unit, size, small, title, code) = (self.unit, self.font_size, self.small_size, self.title_size, self.code_size);
        ctx.global_style_mut(|style| {
            style.text_styles = [
                (TextStyle::Small, FontId::proportional(small)),
                (TextStyle::Body, FontId::proportional(size)),
                (TextStyle::Button, FontId::proportional(size)),
                (TextStyle::Heading, FontId::new(title, crate::fonts::semibold())),
                (TextStyle::Monospace, FontId::monospace(code)),
            ]
            .into();
            let s = &mut style.spacing;
            s.item_spacing = egui::vec2(2.0 * unit, 1.5 * unit);
            s.button_padding = egui::vec2(3.0 * unit, 1.5 * unit);
            s.interact_size.y = 7.0 * unit;
            s.menu_margin = Margin::same(6);
            s.window_margin = Margin::same(16);
            s.icon_width = 14.0;
            s.icon_spacing = 2.0 * unit;
            s.slider_width = 45.0 * unit;
            style.animation_time = 0.08;
            style.interaction.selectable_labels = false;
        });
    }
}

/// `over` drawn on `base`.
pub fn blend(base: Color32, over: Color32) -> Color32 {
    let a = f32::from(over.a()) / 255.0;
    let mix = |b: u8, o: u8| (f32::from(b) * (1.0 - a) + f32::from(o) * a).round() as u8;
    Color32::from_rgb(mix(base.r(), over.r()), mix(base.g(), over.g()), mix(base.b(), over.b()))
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
