use super::*;
use std::path::{Path, PathBuf};

pub(super) fn install_cjk_font_fallbacks(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded_fonts = Vec::new();

    for path in cjk_font_candidates() {
        let Some(font_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if fonts.font_data.contains_key(font_name) {
            continue;
        }

        match std::fs::read(&path) {
            Ok(data) => {
                let font_name = font_name.to_owned();
                fonts.font_data.insert(
                    font_name.clone(),
                    std::sync::Arc::new(egui::FontData::from_owned(data)),
                );

                if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                    family.push(font_name.clone());
                }
                if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                    family.push(font_name.clone());
                }

                loaded_fonts.push(path);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => tracing::debug!("failed to load font {}: {}", path.display(), err),
        }
    }

    if loaded_fonts.is_empty() {
        tracing::debug!("no CJK fallback fonts found on the system");
        return;
    }

    ctx.set_fonts(fonts);
    tracing::info!(
        "loaded CJK fallback fonts: {}",
        loaded_fonts
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn cjk_font_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    extend_if_exists(
        &mut candidates,
        dirs::home_dir(),
        &[
            ".local/share/fonts/NotoSansCJK-Regular.ttc",
            ".local/share/fonts/NotoSerifCJK-Regular.ttc",
            ".local/share/fonts/SourceHanSansSC-Regular.otf",
            ".local/share/fonts/SourceHanSansCN-Regular.otf",
            ".local/share/fonts/SourceHanSansJP-Regular.otf",
            ".local/share/fonts/SourceHanSansKR-Regular.otf",
            ".local/share/fonts/SourceHanSansTW-Regular.otf",
            ".local/share/fonts/NanumGothic.ttf",
            ".local/share/fonts/wqy-zenhei.ttc",
            ".fonts/NotoSansCJK-Regular.ttc",
            ".fonts/NotoSerifCJK-Regular.ttc",
            ".fonts/SourceHanSansSC-Regular.otf",
            ".fonts/SourceHanSansCN-Regular.otf",
            ".fonts/SourceHanSansJP-Regular.otf",
            ".fonts/SourceHanSansKR-Regular.otf",
            ".fonts/SourceHanSansTW-Regular.otf",
            ".fonts/NanumGothic.ttf",
            ".fonts/wqy-zenhei.ttc",
        ],
    );

    #[cfg(target_os = "linux")]
    extend_if_exists(
        &mut candidates,
        None,
        &[
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSerifCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansJP-Regular.otf",
            "/usr/share/fonts/opentype/noto/NotoSansKR-Regular.otf",
            "/usr/share/fonts/opentype/noto/NotoSansSC-Regular.otf",
            "/usr/share/fonts/opentype/noto/NotoSansTC-Regular.otf",
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansSC-Regular.otf",
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansCN-Regular.otf",
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansJP-Regular.otf",
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansKR-Regular.otf",
            "/usr/share/fonts/opentype/adobe-source-han-sans/SourceHanSansTW-Regular.otf",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
            "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
            "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
            "/usr/share/fonts/opentype/ipafont-mincho/ipam.ttf",
        ],
    );

    #[cfg(target_os = "macos")]
    extend_if_exists(
        &mut candidates,
        None,
        &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/Supplemental/Songti.ttc",
        ],
    );

    #[cfg(target_os = "windows")]
    {
        let windows_dir = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let local_fonts_dir =
            dirs::data_local_dir().map(|dir| dir.join(r"Microsoft\Windows\Fonts"));

        extend_if_exists(
            &mut candidates,
            Some(windows_dir),
            &[
                r"Fonts\msyh.ttc",
                r"Fonts\msyh.ttf",
                r"Fonts\msyhbd.ttc",
                r"Fonts\YuGothR.ttc",
                r"Fonts\YuGothM.ttc",
                r"Fonts\meiryo.ttc",
                r"Fonts\msgothic.ttc",
                r"Fonts\malgun.ttf",
                r"Fonts\simsun.ttc",
            ],
        );
        extend_if_exists(
            &mut candidates,
            local_fonts_dir,
            &[
                "msyh.ttc",
                "msyh.ttf",
                "msyhbd.ttc",
                "YuGothR.ttc",
                "YuGothM.ttc",
                "meiryo.ttc",
                "msgothic.ttc",
                "malgun.ttf",
                "simsun.ttc",
            ],
        );
    }

    dedupe_paths(candidates)
}

fn extend_if_exists(target: &mut Vec<PathBuf>, base: Option<PathBuf>, suffixes: &[&str]) {
    for suffix in suffixes {
        let path = base
            .as_ref()
            .map(|dir| dir.join(suffix))
            .unwrap_or_else(|| PathBuf::from(suffix));
        if path_exists(&path) {
            target.push(path);
        }
    }
}

fn path_exists(path: &Path) -> bool {
    std::fs::metadata(path).is_ok()
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.iter().any(|existing| existing == &path) {
            deduped.push(path);
        }
    }
    deduped
}
