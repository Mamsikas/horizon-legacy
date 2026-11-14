use crate::{lang, settings::Settings, text::Text};

/// `Options.preferredGraphicsBackend`'s tooltip: the restart note once the choice differs from
/// the startup one, the description, and Vulkan's warning.
pub(super) fn graphics_api_tooltip(s: &Settings) -> Text {
    let line = |key: &str| lang::get_or(key, key);
    let mut lines = Vec::new();
    if s.horizon.api.selected() != s.startup_api.selected() {
        lines.push(line("options.needsRestart"));
        lines.push(String::new());
    }
    lines.push(line("options.graphicsApi.tooltip"));
    if s.horizon.api.selected() == crate::graphics::GraphicsApi::Vulkan {
        lines.push(String::new());
        lines.push(line("options.graphicsApi.tooltip.vulkan"));
    }
    Text::literal(lines.join("\n"))
}
