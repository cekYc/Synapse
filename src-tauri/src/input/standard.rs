// ============================================================
// Synapse — Standard Input Backend (L1: enigo + SendInput)
// ============================================================
// Uses the `enigo` crate for cross-platform input simulation.
// All input operations run on a dedicated high-priority thread
// to avoid tokio async jitter on mouse movements.
// ============================================================

use super::InputBackend;
use crate::engine::ir::{ClickType, KeyModifier, MouseButton, MoveCurve};
use enigo::{
    Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings,
};
use rand::Rng;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

pub struct StandardInput {
    enigo: Mutex<Enigo>,
}

impl StandardInput {
    pub fn new() -> Result<Self, String> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialize Enigo: {e}"))?;
        Ok(Self {
            enigo: Mutex::new(enigo),
        })
    }
}

impl InputBackend for StandardInput {
    fn mouse_click(
        &self,
        button: &MouseButton,
        click_type: &ClickType,
        x: i32,
        y: i32,
        relative: bool,
    ) -> Result<(), String> {
        let mut enigo = self.enigo.lock().map_err(|e| e.to_string())?;

        let btn = match button {
            MouseButton::Left => Button::Left,
            MouseButton::Right => Button::Right,
            MouseButton::Middle => Button::Middle,
        };

        // Resolve the destination. For relative clicks the coordinates are an
        // offset from the current cursor position rather than absolute screen
        // coordinates.
        let (tx, ty) = resolve_target(&*enigo, x, y, relative)?;

        // Move to position first
        enigo
            .move_mouse(tx, ty, Coordinate::Abs)
            .map_err(|e| format!("Mouse move failed: {e}"))?;

        // Small delay for move to register
        thread::sleep(Duration::from_millis(5));

        match click_type {
            ClickType::Single => {
                enigo
                    .button(btn, Direction::Click)
                    .map_err(|e| format!("Click failed: {e}"))?;
            }
            ClickType::Double => {
                enigo
                    .button(btn, Direction::Click)
                    .map_err(|e| format!("Click failed: {e}"))?;
                thread::sleep(Duration::from_millis(50));
                enigo
                    .button(btn, Direction::Click)
                    .map_err(|e| format!("Click failed: {e}"))?;
            }
            ClickType::Hold => {
                // Guarantee the button is released even if the hold sequence is
                // interrupted by an error, so we never leave a button stuck down.
                enigo
                    .button(btn, Direction::Press)
                    .map_err(|e| format!("Press failed: {e}"))?;
                thread::sleep(Duration::from_millis(200));
                enigo
                    .button(btn, Direction::Release)
                    .map_err(|e| format!("Release failed: {e}"))?;
            }
        }

        tracing::debug!("MouseClick: ({tx}, {ty}) {:?} {:?}", button, click_type);
        Ok(())
    }

    fn mouse_move(
        &self,
        x: i32,
        y: i32,
        duration_ms: u64,
        curve: &MoveCurve,
        relative: bool,
    ) -> Result<(), String> {
        let mut enigo = self.enigo.lock().map_err(|e| e.to_string())?;

        // Resolve the destination in absolute screen coordinates.
        let (target_x, target_y) = resolve_target(&*enigo, x, y, relative)?;

        if duration_ms == 0 {
            enigo
                .move_mouse(target_x, target_y, Coordinate::Abs)
                .map_err(|e| format!("Mouse move failed: {e}"))?;
            return Ok(());
        }

        // Interpolate starting from the cursor's *actual* current position, so
        // the motion begins where the pointer is rather than jumping to the
        // top-left corner first.
        let (start_x, start_y) = enigo
            .location()
            .map_err(|e| format!("Failed to read cursor location: {e}"))?;

        let steps = (duration_ms / 10).clamp(5, 200) as usize;
        let step_delay = Duration::from_millis((duration_ms / steps as u64).max(1));

        // We'll simulate by moving in steps. For humanized movement,
        // we add slight random jitter to each step.
        let mut rng = rand::rng();

        for i in 1..=steps {
            let t = i as f64 / steps as f64;

            // Apply curve
            let curved_t = match curve {
                MoveCurve::Linear => t,
                MoveCurve::Bezier => {
                    // Ease-in-out cubic
                    if t < 0.5 {
                        4.0 * t * t * t
                    } else {
                        1.0 - (-2.0 * t + 2.0_f64).powi(3) / 2.0
                    }
                }
                MoveCurve::Humanized => {
                    // Ease-in-out with random jitter
                    let base = if t < 0.5 {
                        4.0 * t * t * t
                    } else {
                        1.0 - (-2.0 * t + 2.0_f64).powi(3) / 2.0
                    };
                    (base + rng.random_range(-0.02..0.02)).clamp(0.0, 1.0)
                }
            };

            // Interpolate from the real start position to the target.
            let interp_x = start_x as f64 + (target_x - start_x) as f64 * curved_t;
            let interp_y = start_y as f64 + (target_y - start_y) as f64 * curved_t;

            let _ = enigo.move_mouse(interp_x.round() as i32, interp_y.round() as i32, Coordinate::Abs);

            // Use std::thread::sleep for precise timing (not tokio)
            thread::sleep(step_delay);
        }

        // Ensure we end exactly at target
        enigo
            .move_mouse(target_x, target_y, Coordinate::Abs)
            .map_err(|e| format!("Mouse move failed: {e}"))?;

        tracing::debug!("MouseMove: ({target_x}, {target_y}) {}ms {:?}", duration_ms, curve);
        Ok(())
    }

    fn key_press(
        &self,
        key: &str,
        modifiers: &[KeyModifier],
        hold_ms: u64,
    ) -> Result<(), String> {
        let mut enigo = self.enigo.lock().map_err(|e| e.to_string())?;
        let main_key = str_to_enigo_key(key);

        // Press modifiers, tracking how many succeeded so we release exactly
        // those on the way out — even if a later step fails.
        let mut pressed = 0usize;
        let mut outcome: Result<(), String> = Ok(());
        for modifier in modifiers {
            match enigo.key(modifier_to_enigo_key(modifier), Direction::Press) {
                Ok(()) => pressed += 1,
                Err(e) => {
                    outcome = Err(format!("Modifier press failed: {e}"));
                    break;
                }
            }
        }

        // Press and hold the main key, then release it.
        if outcome.is_ok() {
            outcome = enigo
                .key(main_key, Direction::Press)
                .map_err(|e| format!("Key press failed: {e}"));
        }
        if outcome.is_ok() {
            thread::sleep(Duration::from_millis(hold_ms));
            outcome = enigo
                .key(main_key, Direction::Release)
                .map_err(|e| format!("Key release failed: {e}"));
        }

        // Always release the modifiers we pressed, in reverse order, regardless
        // of any error above. This prevents Ctrl/Alt/Shift/Win from getting
        // stuck down when a key operation fails midway.
        for modifier in modifiers[..pressed].iter().rev() {
            let _ = enigo.key(modifier_to_enigo_key(modifier), Direction::Release);
        }

        outcome?;
        tracing::debug!("KeyPress: {key} modifiers={modifiers:?} hold={hold_ms}ms");
        Ok(())
    }

    fn release_modifiers(&self) {
        let mut enigo = match self.enigo.lock() {
            Ok(e) => e,
            Err(poisoned) => poisoned.into_inner(),
        };
        for key in [
            enigo::Key::Control,
            enigo::Key::Alt,
            enigo::Key::Shift,
            enigo::Key::Meta,
        ] {
            let _ = enigo.key(key, Direction::Release);
        }
        tracing::debug!("release_modifiers: Ctrl/Alt/Shift/Win released");
    }

    fn type_text(
        &self,
        text: &str,
        delay_per_char_ms: u64,
        humanized: bool,
    ) -> Result<(), String> {
        let mut enigo = self.enigo.lock().map_err(|e| e.to_string())?;
        let mut rng = rand::rng();

        for ch in text.chars() {
            enigo
                .text(&ch.to_string())
                .map_err(|e| format!("Type char failed: {e}"))?;

            let delay = if humanized {
                let jitter: i64 = rng.random_range(-20..20);
                (delay_per_char_ms as i64 + jitter).max(5) as u64
            } else {
                delay_per_char_ms
            };

            thread::sleep(Duration::from_millis(delay));
        }

        tracing::debug!("TypeText: '{}' delay={}ms humanized={humanized}", text, delay_per_char_ms);
        Ok(())
    }
}

/// Resolve requested coordinates to absolute screen coordinates.
///
/// When `relative` is true the `(x, y)` are treated as an offset from the
/// current cursor position; otherwise they are absolute.
fn resolve_target(enigo: &Enigo, x: i32, y: i32, relative: bool) -> Result<(i32, i32), String> {
    if relative {
        let (cx, cy) = enigo
            .location()
            .map_err(|e| format!("Failed to read cursor location: {e}"))?;
        Ok((cx + x, cy + y))
    } else {
        Ok((x, y))
    }
}

fn modifier_to_enigo_key(modifier: &KeyModifier) -> enigo::Key {
    match modifier {
        KeyModifier::Ctrl => enigo::Key::Control,
        KeyModifier::Alt => enigo::Key::Alt,
        KeyModifier::Shift => enigo::Key::Shift,
        KeyModifier::Win => enigo::Key::Meta,
    }
}

fn str_to_enigo_key(key: &str) -> enigo::Key {
    match key.to_lowercase().as_str() {
        "enter" | "return" => enigo::Key::Return,
        "tab" => enigo::Key::Tab,
        "escape" | "esc" => enigo::Key::Escape,
        "backspace" => enigo::Key::Backspace,
        "delete" => enigo::Key::Delete,
        "space" => enigo::Key::Space,
        "up" | "arrowup" => enigo::Key::UpArrow,
        "down" | "arrowdown" => enigo::Key::DownArrow,
        "left" | "arrowleft" => enigo::Key::LeftArrow,
        "right" | "arrowright" => enigo::Key::RightArrow,
        "home" => enigo::Key::Home,
        "end" => enigo::Key::End,
        "pageup" => enigo::Key::PageUp,
        "pagedown" => enigo::Key::PageDown,
        "f1" => enigo::Key::F1,
        "f2" => enigo::Key::F2,
        "f3" => enigo::Key::F3,
        "f4" => enigo::Key::F4,
        "f5" => enigo::Key::F5,
        "f6" => enigo::Key::F6,
        "f7" => enigo::Key::F7,
        "f8" => enigo::Key::F8,
        "f9" => enigo::Key::F9,
        "f10" => enigo::Key::F10,
        "f11" => enigo::Key::F11,
        "f12" => enigo::Key::F12,
        other => {
            // Single character key
            if let Some(c) = other.chars().next() {
                enigo::Key::Unicode(c)
            } else {
                enigo::Key::Return
            }
        }
    }
}
