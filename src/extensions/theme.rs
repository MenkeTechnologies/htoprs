use crossterm::style::Color;
use serde::{Deserialize, Serialize};

/// All named color themes — ported from storageshower.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeName {
    /// `NeonSprawl` variant.
    #[default]
    NeonSprawl,
    /// `AcidRain` variant.
    AcidRain,
    /// `IceBreaker` variant.
    IceBreaker,
    /// `SynthWave` variant.
    SynthWave,
    /// `RustBelt` variant.
    RustBelt,
    /// `GhostWire` variant.
    GhostWire,
    /// `RedSector` variant.
    RedSector,
    /// `SakuraDen` variant.
    SakuraDen,
    /// `DataStream` variant.
    DataStream,
    /// `SolarFlare` variant.
    SolarFlare,
    /// `NeonNoir` variant.
    NeonNoir,
    /// `ChromeHeart` variant.
    ChromeHeart,
    /// `BladeRunner` variant.
    BladeRunner,
    /// `VoidWalker` variant.
    VoidWalker,
    /// `ToxicWaste` variant.
    ToxicWaste,
    /// `CyberFrost` variant.
    CyberFrost,
    /// `PlasmaCore` variant.
    PlasmaCore,
    /// `SteelNerve` variant.
    SteelNerve,
    /// `DarkSignal` variant.
    DarkSignal,
    /// `GlitchPop` variant.
    GlitchPop,
    /// `HoloShift` variant.
    HoloShift,
    /// `NightCity` variant.
    NightCity,
    /// `DeepNet` variant.
    DeepNet,
    /// `LaserGrid` variant.
    LaserGrid,
    /// `QuantumFlux` variant.
    QuantumFlux,
    /// `BioHazard` variant.
    BioHazard,
    /// `Darkwave` variant.
    Darkwave,
    /// `Overlock` variant.
    Overlock,
    /// `Megacorp` variant.
    Megacorp,
    /// `Zaibatsu` variant.
    Zaibatsu,
    /// `Iftopcolor` variant.
    Iftopcolor,
}

impl ThemeName {
    /// Every variant in declaration order — used for `--list-colors`
    /// and config-file enumeration.
    pub const ALL: &'static [ThemeName] = &[
        ThemeName::NeonSprawl,
        ThemeName::AcidRain,
        ThemeName::IceBreaker,
        ThemeName::SynthWave,
        ThemeName::RustBelt,
        ThemeName::GhostWire,
        ThemeName::RedSector,
        ThemeName::SakuraDen,
        ThemeName::DataStream,
        ThemeName::SolarFlare,
        ThemeName::NeonNoir,
        ThemeName::ChromeHeart,
        ThemeName::BladeRunner,
        ThemeName::VoidWalker,
        ThemeName::ToxicWaste,
        ThemeName::CyberFrost,
        ThemeName::PlasmaCore,
        ThemeName::SteelNerve,
        ThemeName::DarkSignal,
        ThemeName::GlitchPop,
        ThemeName::HoloShift,
        ThemeName::NightCity,
        ThemeName::DeepNet,
        ThemeName::LaserGrid,
        ThemeName::QuantumFlux,
        ThemeName::BioHazard,
        ThemeName::Darkwave,
        ThemeName::Overlock,
        ThemeName::Megacorp,
        ThemeName::Zaibatsu,
        ThemeName::Iftopcolor,
    ];
    /// `display_name` — see implementation.
    pub fn display_name(self) -> &'static str {
        match self {
            ThemeName::NeonSprawl => "Neon Sprawl",
            ThemeName::AcidRain => "Acid Rain",
            ThemeName::IceBreaker => "Ice Breaker",
            ThemeName::SynthWave => "Synth Wave",
            ThemeName::RustBelt => "Rust Belt",
            ThemeName::GhostWire => "Ghost Wire",
            ThemeName::RedSector => "Red Sector",
            ThemeName::SakuraDen => "Sakura Den",
            ThemeName::DataStream => "Data Stream",
            ThemeName::SolarFlare => "Solar Flare",
            ThemeName::NeonNoir => "Neon Noir",
            ThemeName::ChromeHeart => "Chrome Heart",
            ThemeName::BladeRunner => "Blade Runner",
            ThemeName::VoidWalker => "Void Walker",
            ThemeName::ToxicWaste => "Toxic Waste",
            ThemeName::CyberFrost => "Cyber Frost",
            ThemeName::PlasmaCore => "Plasma Core",
            ThemeName::SteelNerve => "Steel Nerve",
            ThemeName::DarkSignal => "Dark Signal",
            ThemeName::GlitchPop => "Glitch Pop",
            ThemeName::HoloShift => "Holo Shift",
            ThemeName::NightCity => "Night City",
            ThemeName::DeepNet => "Deep Net",
            ThemeName::LaserGrid => "Laser Grid",
            ThemeName::QuantumFlux => "Quantum Flux",
            ThemeName::BioHazard => "Bio Hazard",
            ThemeName::Darkwave => "Darkwave",
            ThemeName::Overlock => "Overlock",
            ThemeName::Megacorp => "Megacorp",
            ThemeName::Zaibatsu => "Zaibatsu",
            ThemeName::Iftopcolor => "iftopcolor",
        }
    }
}

/// 6-color palette from storageshower: (primary, accent, c3, c4, c5, c6)
fn palette(name: ThemeName) -> (u8, u8, u8, u8, u8, u8) {
    match name {
        ThemeName::NeonSprawl => (27, 48, 135, 141, 63, 99),
        ThemeName::AcidRain => (28, 46, 34, 40, 22, 35),
        ThemeName::IceBreaker => (19, 39, 25, 33, 21, 32),
        ThemeName::SynthWave => (91, 177, 128, 134, 93, 97),
        ThemeName::RustBelt => (172, 214, 178, 220, 166, 130),
        ThemeName::GhostWire => (37, 50, 44, 87, 30, 23),
        ThemeName::RedSector => (160, 203, 196, 210, 124, 88),
        ThemeName::SakuraDen => (175, 218, 182, 225, 169, 132),
        ThemeName::DataStream => (22, 46, 28, 119, 34, 22),
        ThemeName::SolarFlare => (202, 220, 196, 213, 160, 125),
        ThemeName::NeonNoir => (201, 231, 93, 219, 57, 53),
        ThemeName::ChromeHeart => (250, 255, 246, 253, 243, 239),
        ThemeName::BladeRunner => (208, 37, 166, 73, 130, 23),
        ThemeName::VoidWalker => (55, 99, 54, 141, 92, 17),
        ThemeName::ToxicWaste => (118, 190, 154, 226, 82, 58),
        ThemeName::CyberFrost => (159, 195, 153, 189, 111, 67),
        ThemeName::PlasmaCore => (199, 213, 163, 207, 126, 89),
        ThemeName::SteelNerve => (68, 110, 60, 146, 24, 236),
        ThemeName::DarkSignal => (30, 43, 23, 79, 29, 16),
        ThemeName::GlitchPop => (201, 51, 226, 47, 196, 21),
        ThemeName::HoloShift => (123, 219, 159, 183, 87, 133),
        ThemeName::NightCity => (214, 227, 209, 223, 172, 94),
        ThemeName::DeepNet => (19, 33, 17, 75, 26, 16),
        ThemeName::LaserGrid => (46, 201, 51, 226, 196, 21),
        ThemeName::QuantumFlux => (135, 75, 171, 111, 98, 61),
        ThemeName::BioHazard => (148, 184, 106, 192, 64, 22),
        ThemeName::Darkwave => (53, 140, 89, 176, 127, 52),
        ThemeName::Overlock => (196, 208, 160, 214, 124, 52),
        ThemeName::Megacorp => (252, 39, 245, 81, 242, 236),
        ThemeName::Zaibatsu => (167, 216, 131, 224, 95, 52),
        ThemeName::Iftopcolor => (21, 46, 28, 48, 33, 19),
    }
}

/// Complete theme for the iftoprs UI, derived from a 6-color palette.
#[derive(Debug, Clone)]
pub struct Theme {
    /// `bar_color` field.
    pub bar_color: Color,
    /// `bar_color_mid` field.
    pub bar_color_mid: Color,
    /// `bar_text` field.
    pub bar_text: Color,
    /// `host_src` field.
    pub host_src: Color,
    /// `host_dst` field.
    pub host_dst: Color,
    /// `arrow` field.
    pub arrow: Color,
    /// `rate_2s` field.
    pub rate_2s: Color,
    /// `rate_10s` field.
    pub rate_10s: Color,
    /// `rate_40s` field.
    pub rate_40s: Color,
    /// `scale_label` field.
    pub scale_label: Color,
    /// `scale_line` field.
    pub scale_line: Color,
    /// `total_label` field.
    pub total_label: Color,
    /// `cum_label` field.
    pub cum_label: Color,
    /// `peak_label` field.
    pub peak_label: Color,
    /// `proc_name` field.
    pub proc_name: Color,
    /// `help_bg` field.
    pub help_bg: Color,
    /// `help_border` field.
    pub help_border: Color,
    /// `help_title` field.
    pub help_title: Color,
    /// `help_section` field.
    pub help_section: Color,
    /// `help_key` field.
    pub help_key: Color,
    /// `help_val` field.
    pub help_val: Color,
    /// `select_bg` field.
    pub select_bg: Color,
}

/// Custom theme colors stored in config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomThemeColors {
    /// `c1` field.
    pub c1: u8,
    /// `c2` field.
    pub c2: u8,
    /// `c3` field.
    pub c3: u8,
    /// `c4` field.
    pub c4: u8,
    /// `c5` field.
    pub c5: u8,
    /// `c6` field.
    pub c6: u8,
}

impl Theme {
    /// `from_name` — see implementation.
    pub fn from_name(name: ThemeName) -> Self {
        let (c1, c2, c3, c4, c5, c6) = palette(name);
        Self::from_palette_raw(c1, c2, c3, c4, c5, c6)
    }
    /// `from_palette_raw` — see implementation.
    pub fn from_palette_raw(c1: u8, c2: u8, c3: u8, c4: u8, c5: u8, c6: u8) -> Self {
        Theme {
            bar_color: Color::AnsiValue(c6), // darkest — bar background
            bar_color_mid: Color::AnsiValue(Self::shift_color_lighter(c6)),
            bar_text: Color::Black,
            host_src: Color::AnsiValue(c2), // accent — bright
            host_dst: Color::AnsiValue(c2),
            arrow: Color::AnsiValue(c5),
            rate_2s: Color::AnsiValue(c2),  // accent
            rate_10s: Color::AnsiValue(c4), // mid
            rate_40s: Color::AnsiValue(c5), // dim
            scale_label: Color::AnsiValue(c2),
            scale_line: Color::AnsiValue(c6),
            total_label: Color::AnsiValue(c1), // primary
            cum_label: Color::AnsiValue(c2),
            peak_label: Color::AnsiValue(c4),
            proc_name: Color::AnsiValue(c3),
            help_bg: Color::AnsiValue(236),
            help_border: Color::AnsiValue(c1),
            help_title: Color::AnsiValue(c1),
            help_section: Color::AnsiValue(c6),
            help_key: Color::AnsiValue(c2),
            help_val: Color::AnsiValue(c4),
            select_bg: Color::AnsiValue(236),
        }
    }

    /// Shift an indexed 256-color one step lighter in the color cube or grayscale ramp.
    fn shift_color_lighter(c: u8) -> u8 {
        if c >= 232 {
            // Grayscale ramp (232..=255): bump up but clamp at 255. Pre-fix
            // `c + 2` panicked in debug for c == 254 (u8 overflow) and wrapped
            // to a dark color in release for c == 254 or 255.
            c.saturating_add(2)
        } else if c >= 16 {
            // 6x6x6 color cube (16..=231)
            let idx = c - 16;
            let b = idx % 6;
            let g = (idx / 6) % 6;
            let r = idx / 36;
            let r2 = (r + 1).min(5);
            let g2 = (g + 1).min(5);
            let b2 = (b + 1).min(5);
            16 + r2 * 36 + g2 * 6 + b2
        } else {
            // Basic 16 colors — bump to a lighter variant if possible. `c + 8`
            // for c >= 248 would also overflow u8 in debug; use saturating_add.
            c.saturating_add(8).min(15)
        }
    }

    /// Get the raw 6-color palette values for a built-in theme.
    pub fn palette_values(name: ThemeName) -> [u8; 6] {
        let (c1, c2, c3, c4, c5, c6) = palette(name);
        [c1, c2, c3, c4, c5, c6]
    }

    /// Generate a 6-cell color swatch string for theme preview.
    pub fn swatch(name: ThemeName) -> Vec<(Color, &'static str)> {
        let (c1, c2, c3, c4, c5, c6) = palette(name);
        vec![
            (Color::AnsiValue(c1), "██"),
            (Color::AnsiValue(c2), "██"),
            (Color::AnsiValue(c3), "██"),
            (Color::AnsiValue(c4), "██"),
            (Color::AnsiValue(c5), "██"),
            (Color::AnsiValue(c6), "██"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn all_themes_count() {
        assert_eq!(ThemeName::ALL.len(), 31);
    }

    #[test]
    fn default_theme_is_neon_sprawl() {
        assert_eq!(ThemeName::default(), ThemeName::NeonSprawl);
    }

    #[test]
    fn all_themes_have_display_names() {
        for &name in ThemeName::ALL {
            assert!(!name.display_name().is_empty());
        }
    }

    #[test]
    fn all_themes_produce_valid_theme() {
        for &name in ThemeName::ALL {
            let _theme = Theme::from_name(name);
        }
    }

    #[test]
    fn swatch_has_six_colors() {
        for &name in ThemeName::ALL {
            assert_eq!(Theme::swatch(name).len(), 6);
        }
    }

    #[test]
    fn theme_fields_are_indexed_colors() {
        let t = Theme::from_name(ThemeName::NeonSprawl);
        assert!(matches!(t.bar_color, Color::AnsiValue(_)));
        assert_eq!(t.bar_text, Color::Black);
        assert!(matches!(t.host_src, Color::AnsiValue(_)));
        assert!(matches!(t.arrow, Color::AnsiValue(_)));
        assert!(matches!(t.proc_name, Color::AnsiValue(_)));
        assert_eq!(t.help_bg, Color::AnsiValue(236));
    }

    #[test]
    fn all_themes_unique_display_names() {
        let mut names: Vec<&str> = ThemeName::ALL.iter().map(|t| t.display_name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), ThemeName::ALL.len());
    }

    #[test]
    fn neon_sprawl_palette() {
        let t = Theme::from_name(ThemeName::NeonSprawl);
        assert_eq!(t.bar_color, Color::AnsiValue(99));
        assert_eq!(t.host_src, Color::AnsiValue(48));
        assert_eq!(t.total_label, Color::AnsiValue(27));
        assert_eq!(t.proc_name, Color::AnsiValue(135));
    }

    #[test]
    fn blade_runner_palette() {
        let t = Theme::from_name(ThemeName::BladeRunner);
        assert_eq!(t.bar_color, Color::AnsiValue(23));
        assert_eq!(t.host_src, Color::AnsiValue(37));
        assert_eq!(t.total_label, Color::AnsiValue(208));
    }

    #[test]
    fn swatch_colors_match_palette() {
        let s = Theme::swatch(ThemeName::NeonSprawl);
        assert_eq!(s[0].0, Color::AnsiValue(27));
        assert_eq!(s[1].0, Color::AnsiValue(48));
        assert_eq!(s[5].0, Color::AnsiValue(99));
    }

    #[test]
    fn theme_name_serde_roundtrip() {
        let name = ThemeName::BladeRunner;
        let json = serde_json::to_string(&name).unwrap();
        let parsed: ThemeName = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, name);
    }

    #[test]
    fn theme_all_contains_default() {
        assert!(ThemeName::ALL.contains(&ThemeName::default()));
    }

    #[test]
    fn theme_clone() {
        let t = Theme::from_name(ThemeName::AcidRain);
        let t2 = t.clone();
        assert_eq!(t.bar_color, t2.bar_color);
        assert_eq!(t.host_src, t2.host_src);
    }

    #[test]
    fn theme_name_json_roundtrip_all_variants() {
        for &name in ThemeName::ALL {
            let json = serde_json::to_string(&name).unwrap();
            let back: ThemeName = serde_json::from_str(&json).unwrap();
            assert_eq!(back, name);
        }
    }

    #[test]
    fn theme_from_name_zaibatsu_has_indexed_colors() {
        let t = Theme::from_name(ThemeName::Zaibatsu);
        assert!(matches!(t.bar_color, Color::AnsiValue(_)));
    }

    #[test]
    fn display_name_never_equals_debug_string() {
        let n = ThemeName::NeonSprawl;
        assert_ne!(n.display_name(), format!("{:?}", n));
    }

    #[test]
    fn iftopcolor_theme_renders() {
        let t = Theme::from_name(ThemeName::Iftopcolor);
        assert!(matches!(t.bar_color, Color::AnsiValue(_)));
    }

    #[test]
    fn darkwave_theme_arrow_is_indexed() {
        let t = Theme::from_name(ThemeName::Darkwave);
        assert!(matches!(t.arrow, Color::AnsiValue(_)));
    }

    #[test]
    fn steel_nerve_theme_scale_line_is_indexed() {
        let t = Theme::from_name(ThemeName::SteelNerve);
        assert!(matches!(t.scale_line, Color::AnsiValue(_)));
    }

    #[test]
    fn neon_sprawl_theme_host_src_is_indexed() {
        let t = Theme::from_name(ThemeName::NeonSprawl);
        assert!(matches!(t.host_src, Color::AnsiValue(_)));
    }

    #[test]
    fn night_city_theme_total_label_is_indexed() {
        let t = Theme::from_name(ThemeName::NightCity);
        assert!(matches!(t.total_label, Color::AnsiValue(_)));
    }

    #[test]
    fn void_walker_theme_help_border_is_indexed() {
        let t = Theme::from_name(ThemeName::VoidWalker);
        assert!(matches!(t.help_border, Color::AnsiValue(_)));
    }

    #[test]
    fn data_stream_theme_bar_color_is_indexed() {
        let t = Theme::from_name(ThemeName::DataStream);
        assert!(matches!(t.bar_color, Color::AnsiValue(_)));
    }

    #[test]
    fn acid_rain_theme_host_dst_is_indexed() {
        let t = Theme::from_name(ThemeName::AcidRain);
        assert!(matches!(t.host_dst, Color::AnsiValue(_)));
    }

    #[test]
    fn rust_belt_theme_peak_label_is_indexed() {
        let t = Theme::from_name(ThemeName::RustBelt);
        assert!(matches!(t.peak_label, Color::AnsiValue(_)));
    }

    #[test]
    fn solar_flare_theme_scale_label_is_indexed() {
        let t = Theme::from_name(ThemeName::SolarFlare);
        assert!(matches!(t.scale_label, Color::AnsiValue(_)));
    }

    #[test]
    fn toxic_waste_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::ToxicWaste);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn ghost_wire_theme_arrow_is_indexed() {
        let t = Theme::from_name(ThemeName::GhostWire);
        assert!(matches!(t.arrow, Color::AnsiValue(_)));
    }

    #[test]
    fn deep_net_theme_help_border_is_indexed() {
        let t = Theme::from_name(ThemeName::DeepNet);
        assert!(matches!(t.help_border, Color::AnsiValue(_)));
    }

    #[test]
    fn megacorp_theme_swatch_len() {
        assert_eq!(Theme::swatch(ThemeName::Megacorp).len(), 6);
    }

    #[test]
    fn every_theme_name_debug_is_single_token() {
        for &name in ThemeName::ALL {
            let d = format!("{:?}", name);
            assert!(!d.contains(' '), "{name:?}");
        }
    }

    #[test]
    fn zaibatsu_from_name_has_indexed_host_colors() {
        let t = Theme::from_name(ThemeName::Zaibatsu);
        assert!(matches!(t.host_src, Color::AnsiValue(_)));
        assert!(matches!(t.host_dst, Color::AnsiValue(_)));
    }

    #[test]
    fn palette_values_matches_swatch_indexed_colors() {
        for &name in ThemeName::ALL {
            let pal = Theme::palette_values(name);
            let sw = Theme::swatch(name);
            for i in 0..6 {
                assert_eq!(Color::AnsiValue(pal[i]), sw[i].0);
            }
        }
    }

    #[test]
    fn from_palette_raw_zero_grayscale_bar_mid() {
        let t = Theme::from_palette_raw(1, 2, 3, 4, 5, 240);
        assert!(matches!(t.bar_color, Color::AnsiValue(240)));
        assert!(matches!(t.bar_color_mid, Color::AnsiValue(_)));
    }

    #[test]
    fn palette_values_always_six_bytes_per_theme() {
        for &name in ThemeName::ALL {
            assert_eq!(Theme::palette_values(name).len(), 6);
        }
    }

    #[test]
    fn from_name_night_city_help_colors_indexed() {
        let t = Theme::from_name(ThemeName::NightCity);
        assert!(matches!(t.help_title, Color::AnsiValue(_)));
        assert!(matches!(t.help_val, Color::AnsiValue(_)));
    }

    #[test]
    fn from_name_laser_grid_select_bg_is_fixed_gray() {
        let t = Theme::from_name(ThemeName::LaserGrid);
        assert_eq!(t.select_bg, Color::AnsiValue(236));
    }

    #[test]
    fn custom_theme_colors_serde_roundtrip() {
        let c = CustomThemeColors {
            c1: 1,
            c2: 2,
            c3: 3,
            c4: 4,
            c5: 5,
            c6: 6,
        };
        let json = serde_json::to_string(&c).unwrap();
        let c2: CustomThemeColors = serde_json::from_str(&json).unwrap();
        assert_eq!(c2.c3, 3);
    }

    #[test]
    fn theme_from_name_void_walker_rate_colors_differ() {
        let t = Theme::from_name(ThemeName::VoidWalker);
        assert_ne!(t.rate_2s, t.rate_40s);
    }

    #[test]
    fn glitch_pop_theme_bar_and_scale_indexed() {
        let t = Theme::from_name(ThemeName::GlitchPop);
        assert!(matches!(t.bar_color, Color::AnsiValue(_)));
        assert!(matches!(t.scale_line, Color::AnsiValue(_)));
    }

    #[test]
    fn night_city_display_name_has_space() {
        assert!(ThemeName::NightCity.display_name().contains(' '));
    }

    #[test]
    fn plasma_core_bar_text_is_black() {
        let t = Theme::from_name(ThemeName::PlasmaCore);
        assert_eq!(t.bar_text, Color::Black);
    }

    #[test]
    fn custom_theme_colors_all_255_roundtrip() {
        let c = CustomThemeColors {
            c1: 255,
            c2: 255,
            c3: 255,
            c4: 255,
            c5: 255,
            c6: 255,
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: CustomThemeColors = serde_json::from_str(&json).unwrap();
        assert_eq!(back.c6, 255);
    }

    #[test]
    fn megacorp_theme_peak_label_is_indexed() {
        let t = Theme::from_name(ThemeName::Megacorp);
        assert!(matches!(t.peak_label, Color::AnsiValue(_)));
    }

    #[test]
    fn dark_signal_theme_cum_label_is_indexed() {
        let t = Theme::from_name(ThemeName::DarkSignal);
        assert!(matches!(t.cum_label, Color::AnsiValue(_)));
    }

    #[test]
    fn bio_hazard_theme_rate_columns_are_indexed() {
        let t = Theme::from_name(ThemeName::BioHazard);
        assert!(matches!(t.rate_2s, Color::AnsiValue(_)));
        assert!(matches!(t.rate_10s, Color::AnsiValue(_)));
        assert!(matches!(t.rate_40s, Color::AnsiValue(_)));
    }

    #[test]
    fn overlock_theme_scale_label_is_indexed() {
        let t = Theme::from_name(ThemeName::Overlock);
        assert!(matches!(t.scale_label, Color::AnsiValue(_)));
    }

    #[test]
    fn quantum_flux_theme_total_label_is_indexed() {
        let t = Theme::from_name(ThemeName::QuantumFlux);
        assert!(matches!(t.total_label, Color::AnsiValue(_)));
    }

    #[test]
    fn night_city_theme_arrow_is_indexed() {
        let t = Theme::from_name(ThemeName::NightCity);
        assert!(matches!(t.arrow, Color::AnsiValue(_)));
    }

    #[test]
    fn holo_shift_theme_proc_name_is_indexed() {
        let t = Theme::from_name(ThemeName::HoloShift);
        assert!(matches!(t.proc_name, Color::AnsiValue(_)));
    }

    #[test]
    fn deep_net_theme_help_bg_is_indexed() {
        let t = Theme::from_name(ThemeName::DeepNet);
        assert!(matches!(t.help_bg, Color::AnsiValue(_)));
    }

    #[test]
    fn megacorp_theme_bar_color_is_indexed() {
        let t = Theme::from_name(ThemeName::Megacorp);
        assert!(matches!(t.bar_color, Color::AnsiValue(_)));
    }

    #[test]
    fn red_sector_theme_help_title_is_indexed() {
        let t = Theme::from_name(ThemeName::RedSector);
        assert!(matches!(t.help_title, Color::AnsiValue(_)));
    }

    #[test]
    fn sakura_den_theme_bar_color_mid_is_indexed() {
        let t = Theme::from_name(ThemeName::SakuraDen);
        assert!(matches!(t.bar_color_mid, Color::AnsiValue(_)));
    }

    #[test]
    fn iftopcolor_theme_help_section_is_indexed() {
        let t = Theme::from_name(ThemeName::Iftopcolor);
        assert!(matches!(t.help_section, Color::AnsiValue(_)));
    }

    #[test]
    fn chrome_heart_theme_help_val_is_indexed() {
        let t = Theme::from_name(ThemeName::ChromeHeart);
        assert!(matches!(t.help_val, Color::AnsiValue(_)));
    }

    #[test]
    fn synth_wave_theme_rate_10s_is_indexed() {
        let t = Theme::from_name(ThemeName::SynthWave);
        assert!(matches!(t.rate_10s, Color::AnsiValue(_)));
    }

    #[test]
    fn neon_noir_theme_select_bg_is_fixed_gray() {
        let t = Theme::from_name(ThemeName::NeonNoir);
        assert_eq!(t.select_bg, Color::AnsiValue(236));
    }

    #[test]
    fn blade_runner_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::BladeRunner);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn zaibatsu_theme_peak_label_is_indexed() {
        let t = Theme::from_name(ThemeName::Zaibatsu);
        assert!(matches!(t.peak_label, Color::AnsiValue(_)));
    }

    #[test]
    fn laser_grid_theme_cum_label_is_indexed() {
        let t = Theme::from_name(ThemeName::LaserGrid);
        assert!(matches!(t.cum_label, Color::AnsiValue(_)));
    }

    #[test]
    fn void_walker_theme_help_val_is_indexed() {
        let t = Theme::from_name(ThemeName::VoidWalker);
        assert!(matches!(t.help_val, Color::AnsiValue(_)));
    }

    #[test]
    fn plasma_core_theme_peak_label_is_indexed() {
        let t = Theme::from_name(ThemeName::PlasmaCore);
        assert!(matches!(t.peak_label, Color::AnsiValue(_)));
    }

    #[test]
    fn cyber_frost_theme_help_border_is_indexed() {
        let t = Theme::from_name(ThemeName::CyberFrost);
        assert!(matches!(t.help_border, Color::AnsiValue(_)));
    }

    #[test]
    fn ice_breaker_theme_rate_40s_is_indexed() {
        let t = Theme::from_name(ThemeName::IceBreaker);
        assert!(matches!(t.rate_40s, Color::AnsiValue(_)));
    }

    #[test]
    fn neon_noir_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::NeonNoir);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn rust_belt_theme_total_label_is_indexed() {
        let t = Theme::from_name(ThemeName::RustBelt);
        assert!(matches!(t.total_label, Color::AnsiValue(_)));
    }

    #[test]
    fn acid_rain_theme_arrow_is_indexed() {
        let t = Theme::from_name(ThemeName::AcidRain);
        assert!(matches!(t.arrow, Color::AnsiValue(_)));
    }

    #[test]
    fn data_stream_theme_help_title_is_indexed() {
        let t = Theme::from_name(ThemeName::DataStream);
        assert!(matches!(t.help_title, Color::AnsiValue(_)));
    }

    #[test]
    fn darkwave_theme_help_section_is_indexed() {
        let t = Theme::from_name(ThemeName::Darkwave);
        assert!(matches!(t.help_section, Color::AnsiValue(_)));
    }

    #[test]
    fn red_sector_theme_rate_10s_is_indexed() {
        let t = Theme::from_name(ThemeName::RedSector);
        assert!(matches!(t.rate_10s, Color::AnsiValue(_)));
    }

    #[test]
    fn sakura_den_theme_proc_name_is_indexed() {
        let t = Theme::from_name(ThemeName::SakuraDen);
        assert!(matches!(t.proc_name, Color::AnsiValue(_)));
    }

    #[test]
    fn neon_sprawl_theme_help_val_is_indexed() {
        let t = Theme::from_name(ThemeName::NeonSprawl);
        assert!(matches!(t.help_val, Color::AnsiValue(_)));
    }

    #[test]
    fn synth_wave_theme_rate_2s_is_indexed() {
        let t = Theme::from_name(ThemeName::SynthWave);
        assert!(matches!(t.rate_2s, Color::AnsiValue(_)));
    }

    #[test]
    fn blade_runner_theme_rate_40s_is_indexed() {
        let t = Theme::from_name(ThemeName::BladeRunner);
        assert!(matches!(t.rate_40s, Color::AnsiValue(_)));
    }

    #[test]
    fn toxic_waste_theme_help_section_is_indexed() {
        let t = Theme::from_name(ThemeName::ToxicWaste);
        assert!(matches!(t.help_section, Color::AnsiValue(_)));
    }

    #[test]
    fn night_city_theme_scale_line_is_indexed() {
        let t = Theme::from_name(ThemeName::NightCity);
        assert!(matches!(t.scale_line, Color::AnsiValue(_)));
    }

    #[test]
    fn megacorp_theme_help_title_is_indexed() {
        let t = Theme::from_name(ThemeName::Megacorp);
        assert!(matches!(t.help_title, Color::AnsiValue(_)));
    }

    #[test]
    fn laser_grid_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::LaserGrid);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn chrome_heart_theme_scale_line_is_indexed() {
        let t = Theme::from_name(ThemeName::ChromeHeart);
        assert!(matches!(t.scale_line, Color::AnsiValue(_)));
    }

    #[test]
    fn neon_noir_theme_host_src_is_indexed() {
        let t = Theme::from_name(ThemeName::NeonNoir);
        assert!(matches!(t.host_src, Color::AnsiValue(_)));
    }

    #[test]
    fn steel_nerve_theme_cum_label_is_indexed() {
        let t = Theme::from_name(ThemeName::SteelNerve);
        assert!(matches!(t.cum_label, Color::AnsiValue(_)));
    }

    #[test]
    fn dark_signal_theme_scale_line_is_indexed() {
        let t = Theme::from_name(ThemeName::DarkSignal);
        assert!(matches!(t.scale_line, Color::AnsiValue(_)));
    }

    #[test]
    fn ice_breaker_theme_help_title_is_indexed() {
        let t = Theme::from_name(ThemeName::IceBreaker);
        assert!(matches!(t.help_title, Color::AnsiValue(_)));
    }

    #[test]
    fn glitch_pop_theme_host_src_is_indexed() {
        let t = Theme::from_name(ThemeName::GlitchPop);
        assert!(matches!(t.host_src, Color::AnsiValue(_)));
    }

    #[test]
    fn holo_shift_theme_help_border_is_indexed() {
        let t = Theme::from_name(ThemeName::HoloShift);
        assert!(matches!(t.help_border, Color::AnsiValue(_)));
    }

    #[test]
    fn overlock_theme_host_dst_is_indexed() {
        let t = Theme::from_name(ThemeName::Overlock);
        assert!(matches!(t.host_dst, Color::AnsiValue(_)));
    }

    #[test]
    fn bio_hazard_theme_help_section_is_indexed() {
        let t = Theme::from_name(ThemeName::BioHazard);
        assert!(matches!(t.help_section, Color::AnsiValue(_)));
    }

    #[test]
    fn solar_flare_theme_bar_color_mid_is_indexed() {
        let t = Theme::from_name(ThemeName::SolarFlare);
        assert!(matches!(t.bar_color_mid, Color::AnsiValue(_)));
    }

    #[test]
    fn zaibatsu_theme_rate_2s_is_indexed() {
        let t = Theme::from_name(ThemeName::Zaibatsu);
        assert!(matches!(t.rate_2s, Color::AnsiValue(_)));
    }

    #[test]
    fn quantum_flux_theme_help_border_is_indexed() {
        let t = Theme::from_name(ThemeName::QuantumFlux);
        assert!(matches!(t.help_border, Color::AnsiValue(_)));
    }

    #[test]
    fn iftopcolor_theme_peak_label_is_indexed() {
        let t = Theme::from_name(ThemeName::Iftopcolor);
        assert!(matches!(t.peak_label, Color::AnsiValue(_)));
    }

    #[test]
    fn ghost_wire_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::GhostWire);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn red_sector_theme_host_dst_is_indexed() {
        let t = Theme::from_name(ThemeName::RedSector);
        assert!(matches!(t.host_dst, Color::AnsiValue(_)));
    }

    #[test]
    fn sakura_den_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::SakuraDen);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn cyber_frost_theme_help_title_is_indexed() {
        let t = Theme::from_name(ThemeName::CyberFrost);
        assert!(matches!(t.help_title, Color::AnsiValue(_)));
    }

    #[test]
    fn deep_net_theme_cum_label_is_indexed() {
        let t = Theme::from_name(ThemeName::DeepNet);
        assert!(matches!(t.cum_label, Color::AnsiValue(_)));
    }

    #[test]
    fn neon_sprawl_theme_arrow_is_indexed() {
        let t = Theme::from_name(ThemeName::NeonSprawl);
        assert!(matches!(t.arrow, Color::AnsiValue(_)));
    }

    #[test]
    fn plasma_core_theme_help_section_is_indexed() {
        let t = Theme::from_name(ThemeName::PlasmaCore);
        assert!(matches!(t.help_section, Color::AnsiValue(_)));
    }

    #[test]
    fn synth_wave_theme_scale_label_is_indexed() {
        let t = Theme::from_name(ThemeName::SynthWave);
        assert!(matches!(t.scale_label, Color::AnsiValue(_)));
    }

    #[test]
    fn data_stream_theme_proc_name_is_indexed() {
        let t = Theme::from_name(ThemeName::DataStream);
        assert!(matches!(t.proc_name, Color::AnsiValue(_)));
    }

    #[test]
    fn rust_belt_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::RustBelt);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn toxic_waste_theme_rate_40s_is_indexed() {
        let t = Theme::from_name(ThemeName::ToxicWaste);
        assert!(matches!(t.rate_40s, Color::AnsiValue(_)));
    }

    #[test]
    fn blade_runner_theme_host_src_is_indexed() {
        let t = Theme::from_name(ThemeName::BladeRunner);
        assert!(matches!(t.host_src, Color::AnsiValue(_)));
    }

    #[test]
    fn darkwave_theme_rate_10s_is_indexed() {
        let t = Theme::from_name(ThemeName::Darkwave);
        assert!(matches!(t.rate_10s, Color::AnsiValue(_)));
    }

    #[test]
    fn megacorp_theme_help_val_is_indexed() {
        let t = Theme::from_name(ThemeName::Megacorp);
        assert!(matches!(t.help_val, Color::AnsiValue(_)));
    }

    #[test]
    fn acid_rain_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::AcidRain);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn void_walker_theme_rate_10s_is_indexed() {
        let t = Theme::from_name(ThemeName::VoidWalker);
        assert!(matches!(t.rate_10s, Color::AnsiValue(_)));
    }

    #[test]
    fn chrome_heart_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::ChromeHeart);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn synth_wave_theme_help_section_is_indexed() {
        let t = Theme::from_name(ThemeName::SynthWave);
        assert!(matches!(t.help_section, Color::AnsiValue(_)));
    }

    #[test]
    fn neon_noir_theme_host_dst_is_indexed() {
        let t = Theme::from_name(ThemeName::NeonNoir);
        assert!(matches!(t.host_dst, Color::AnsiValue(_)));
    }

    #[test]
    fn ice_breaker_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::IceBreaker);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn zaibatsu_theme_help_section_is_indexed() {
        let t = Theme::from_name(ThemeName::Zaibatsu);
        assert!(matches!(t.help_section, Color::AnsiValue(_)));
    }

    #[test]
    fn laser_grid_theme_host_src_is_indexed() {
        let t = Theme::from_name(ThemeName::LaserGrid);
        assert!(matches!(t.host_src, Color::AnsiValue(_)));
    }

    #[test]
    fn dark_signal_theme_peak_label_is_indexed() {
        let t = Theme::from_name(ThemeName::DarkSignal);
        assert!(matches!(t.peak_label, Color::AnsiValue(_)));
    }

    #[test]
    fn holo_shift_theme_rate_2s_is_indexed() {
        let t = Theme::from_name(ThemeName::HoloShift);
        assert!(matches!(t.rate_2s, Color::AnsiValue(_)));
    }

    #[test]
    fn plasma_core_theme_host_dst_is_indexed() {
        let t = Theme::from_name(ThemeName::PlasmaCore);
        assert!(matches!(t.host_dst, Color::AnsiValue(_)));
    }

    #[test]
    fn bio_hazard_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::BioHazard);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn rust_belt_theme_help_section_is_indexed() {
        let t = Theme::from_name(ThemeName::RustBelt);
        assert!(matches!(t.help_section, Color::AnsiValue(_)));
    }

    #[test]
    fn data_stream_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::DataStream);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn solar_flare_theme_help_title_is_indexed() {
        let t = Theme::from_name(ThemeName::SolarFlare);
        assert!(matches!(t.help_title, Color::AnsiValue(_)));
    }

    #[test]
    fn deep_net_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::DeepNet);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn overlock_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::Overlock);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn iftopcolor_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::Iftopcolor);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn quantum_flux_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::QuantumFlux);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn night_city_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::NightCity);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn megacorp_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::Megacorp);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn synth_wave_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::SynthWave);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn void_walker_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::VoidWalker);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn cyber_frost_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::CyberFrost);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn plasma_core_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::PlasmaCore);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn steel_nerve_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::SteelNerve);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn dark_signal_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::DarkSignal);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn glitch_pop_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::GlitchPop);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn holo_shift_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::HoloShift);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn solar_flare_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::SolarFlare);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn zaibatsu_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::Zaibatsu);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn neon_sprawl_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::NeonSprawl);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn red_sector_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::RedSector);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn darkwave_theme_help_key_is_indexed() {
        let t = Theme::from_name(ThemeName::Darkwave);
        assert!(matches!(t.help_key, Color::AnsiValue(_)));
    }

    #[test]
    fn theme_all_display_names_non_empty() {
        for &t in ThemeName::ALL {
            assert!(!t.display_name().is_empty());
        }
    }

    #[test]
    fn theme_all_display_names_are_unique() {
        let mut seen = HashSet::new();
        for &t in ThemeName::ALL {
            assert!(
                seen.insert(t.display_name()),
                "duplicate display name for {:?}",
                t
            );
        }
    }

    /// `shift_color_lighter` must not panic on the u8 max values reachable via
    /// `CustomThemeColors`. Pre-fix, grayscale 254 / 255 (c + 2 = 256, u8
    /// overflow) and basic 248..255 (c + 8 overflow) crashed in debug.
    /// Test every u8 input to catch any future overflow regression.
    #[test]
    fn shift_color_lighter_no_panic_across_full_u8_range() {
        for c in 0..=255u8 {
            let _ = Theme::shift_color_lighter(c);
        }
    }

    /// Saturation contract: grayscale 254/255 → 255 (lightest, not wrap-around).
    /// Basic palette (<16) caps at 15 (lightest basic color).
    #[test]
    fn shift_color_lighter_saturates_at_palette_ceilings() {
        assert_eq!(Theme::shift_color_lighter(254), 255);
        assert_eq!(Theme::shift_color_lighter(255), 255);
        assert_eq!(Theme::shift_color_lighter(15), 15);
        assert_eq!(Theme::shift_color_lighter(8), 15);
    }
}
