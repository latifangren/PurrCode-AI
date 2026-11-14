// SPDX-License-Identifier: GPL-3.0-or-later

//! Config-driven TUI color palettes and presentation settings.

use purrcode_config::UiConfig;
use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct TuiPalette {
    pub accent: Color,
    pub accent_secondary: Color,
    pub text_primary: Color,
    pub text_muted: Color,
    pub border: Color,
    pub code_fg: Color,
    pub code_bg: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub quote: Color,
}

impl TuiPalette {
    pub fn from_ui_config(cfg: &UiConfig) -> Self {
        let mut palette = match cfg.theme.to_ascii_lowercase().as_str() {
            "catppuccin-mocha" | "mocha" => Self {
                accent: Color::Rgb(203, 166, 247),
                accent_secondary: Color::Rgb(137, 180, 250),
                text_primary: Color::Rgb(205, 214, 244),
                text_muted: Color::Rgb(166, 173, 200),
                border: Color::Rgb(88, 91, 112),
                code_fg: Color::Rgb(166, 227, 161),
                code_bg: Color::Rgb(30, 30, 46),
                success: Color::Rgb(166, 227, 161),
                warning: Color::Rgb(249, 226, 175),
                error: Color::Rgb(243, 139, 168),
                quote: Color::Rgb(148, 226, 213),
            },
            "rose-pine" | "rosepine" => Self {
                accent: Color::Rgb(235, 188, 186),
                accent_secondary: Color::Rgb(196, 167, 231),
                text_primary: Color::Rgb(224, 222, 244),
                text_muted: Color::Rgb(144, 140, 170),
                border: Color::Rgb(68, 65, 90),
                code_fg: Color::Rgb(156, 207, 216),
                code_bg: Color::Rgb(31, 29, 46),
                success: Color::Rgb(49, 116, 143),
                warning: Color::Rgb(246, 193, 119),
                error: Color::Rgb(235, 111, 146),
                quote: Color::Rgb(156, 207, 216),
            },
            "tokyo-night" | "tokyonight" => Self {
                accent: Color::Rgb(122, 162, 247),
                accent_secondary: Color::Rgb(187, 154, 247),
                text_primary: Color::Rgb(192, 202, 245),
                text_muted: Color::Rgb(169, 177, 214),
                border: Color::Rgb(65, 72, 104),
                code_fg: Color::Rgb(115, 218, 202),
                code_bg: Color::Rgb(26, 27, 38),
                success: Color::Rgb(158, 206, 106),
                warning: Color::Rgb(224, 175, 104),
                error: Color::Rgb(247, 118, 142),
                quote: Color::Rgb(125, 207, 255),
            },
            "midnight-slate" | "midnight" => Self {
                accent: Color::Rgb(56, 189, 248),
                accent_secondary: Color::Rgb(129, 140, 248),
                text_primary: Color::Rgb(226, 232, 240),
                text_muted: Color::Rgb(148, 163, 184),
                border: Color::Rgb(51, 65, 85),
                code_fg: Color::Rgb(103, 232, 249),
                code_bg: Color::Rgb(15, 23, 42),
                success: Color::Rgb(52, 211, 153),
                warning: Color::Rgb(251, 191, 36),
                error: Color::Rgb(248, 113, 113),
                quote: Color::Rgb(148, 163, 184),
            },
            _ => Self {
                // Default warm-paper / modern zinc cat palette
                accent: Color::Rgb(249, 115, 22),
                accent_secondary: Color::Rgb(251, 191, 36),
                text_primary: Color::Rgb(244, 244, 245),
                text_muted: Color::Rgb(161, 161, 170),
                border: Color::Rgb(63, 63, 70),
                code_fg: Color::Rgb(134, 239, 172),
                code_bg: Color::Rgb(24, 24, 27),
                success: Color::Rgb(16, 185, 129),
                warning: Color::Rgb(234, 179, 8),
                error: Color::Rgb(239, 68, 68),
                quote: Color::Rgb(161, 161, 170),
            },
        };

        if let Some(hex) = cfg.custom_theme_tokens.get("primary") {
            if let Some(c) = parse_hex_color(hex) {
                palette.accent = c;
            }
        }
        if let Some(hex) = cfg.custom_theme_tokens.get("ink") {
            if let Some(c) = parse_hex_color(hex) {
                palette.text_primary = c;
            }
        }
        if let Some(hex) = cfg.custom_theme_tokens.get("muted") {
            if let Some(c) = parse_hex_color(hex) {
                palette.text_muted = c;
            }
        }
        palette
    }
}

fn parse_hex_color(s: &str) -> Option<Color> {
    let hex = s.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}
