pub mod standard;

use crate::engine::ir::{MouseButton, ClickType, MoveCurve, KeyModifier};

/// Trait for input simulation backends
pub trait InputBackend: Send + Sync {
    fn mouse_click(
        &self,
        button: &MouseButton,
        click_type: &ClickType,
        x: i32,
        y: i32,
        relative: bool,
    ) -> Result<(), String>;

    fn mouse_move(
        &self,
        x: i32,
        y: i32,
        duration_ms: u64,
        curve: &MoveCurve,
        relative: bool,
    ) -> Result<(), String>;

    fn key_press(
        &self,
        key: &str,
        modifiers: &[KeyModifier],
        hold_ms: u64,
    ) -> Result<(), String>;

    fn type_text(
        &self,
        text: &str,
        delay_per_char_ms: u64,
        humanized: bool,
    ) -> Result<(), String>;

    /// Defensively release all modifier keys (Ctrl/Alt/Shift/Win).
    ///
    /// Called by the executor when a flow ends in an error, as a safety net so
    /// a modifier that was left pressed by an interrupted operation does not
    /// stay stuck down after execution stops.
    fn release_modifiers(&self);
}
