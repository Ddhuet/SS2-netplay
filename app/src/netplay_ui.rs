use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::str::FromStr;

use crate::netplay_controls::{Binding, Controllers, ACTION_NAMES};
use crate::netplay_preferences::Preferences;
use font8x8::{UnicodeFonts, BASIC_FONTS};
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Scale, ScaleMode, Window, WindowOptions};

const WINDOW_WIDTH: usize = 720;
const WINDOW_HEIGHT: usize = 480;
const GBA_WIDTH: usize = 240;
const GBA_HEIGHT: usize = 160;
const GBA_SCALE: usize = 3;
const DEFAULT_ADDRESS: &str = "127.0.0.1";
const DEFAULT_PORT: &str = "24872";

const SETUP_TITLE: &str = "Shining Soul II - Rollback Netplay";

const BACKGROUND: u32 = 0x08121f;
const PANEL: u32 = 0x102337;
const PANEL_DARK: u32 = 0x0d1b2c;
const FIELD: u32 = 0x07101b;
const FIELD_FOCUS: u32 = 0x102e45;
const TEXT: u32 = 0xe6f0f6;
const MUTED_TEXT: u32 = 0x9bb1c2;
const ACCENT: u32 = 0x58d6c2;
const ACCENT_DARK: u32 = 0x1d5c61;
const BUTTON: u32 = 0x16505a;
const BUTTON_HOVER: u32 = 0x217477;
const BUTTON_DISABLED: u32 = 0x253746;
const BUTTON_SELECTED_DISABLED: u32 = 0x354957;
const ERROR: u32 = 0xffa07a;

const IP_FIELD: Rect = Rect {
    x: 220,
    y: 126,
    width: 420,
    height: 38,
};
const PORT_FIELD: Rect = Rect {
    x: 220,
    y: 184,
    width: 180,
    height: 38,
};
const HOST_BUTTON: Rect = Rect {
    x: 80,
    y: 250,
    width: 220,
    height: 48,
};
const CONNECT_BUTTON: Rect = Rect {
    x: 330,
    y: 250,
    width: 220,
    height: 48,
};
const DELAY_SELECTOR: Rect = Rect {
    x: 420,
    y: 184,
    width: 220,
    height: 38,
};
const STATUS_PANEL: Rect = Rect {
    x: 80,
    y: 326,
    width: 560,
    height: 68,
};

/// A command emitted by the setup screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Host(u16),
    Connect(SocketAddr),
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Focus {
    Address,
    Port,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Screen {
    Setup,
    Game,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl Rect {
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x as f32
            && x < (self.x + self.width) as f32
            && y >= self.y as f32
            && y < (self.y + self.height) as f32
    }
}

#[derive(Default)]
struct ExitPrompt {
    open: bool,
    mouse_armed: bool,
}

const EXIT_YES: Rect = Rect {
    x: 385,
    y: 258,
    width: 180,
    height: 48,
};
const EXIT_NO: Rect = Rect {
    x: 155,
    y: 258,
    width: 200,
    height: 48,
};

const DEFAULT_LOCAL_DELAY: u32 = 2;
const DEBUG_BACKGROUND: u32 = 0xffffff;
const DEBUG_TEXT: u32 = 0xb00000;
const DEBUG_MAX_COLUMNS: usize = 46;
const DEBUG_MAX_LINES: usize = 12;
const SETTINGS_PANEL: Rect = Rect {
    x: 8,
    y: 146,
    width: 704,
    height: 326,
};
const VOLUME_SLIDER: Rect = Rect {
    x: 164,
    y: 185,
    width: 472,
    height: 24,
};

fn binding_rect(index: usize) -> Rect {
    Rect {
        x: 24 + (index / 5) * 344,
        y: 244 + (index % 5) * 29,
        width: 328,
        height: 25,
    }
}

fn volume_from_mouse(x: f32) -> u8 {
    (((x - VOLUME_SLIDER.x as f32) / (VOLUME_SLIDER.width - 1) as f32).clamp(0.0, 1.0) * 100.0)
        .round() as u8
}

impl ExitPrompt {
    // Only a fresh mouse press after opening may confirm. Keyboard input is
    // consumed by the caller while open; the network loop continues running.
    fn update(&mut self, escape: bool, mouse_down: bool, position: Option<(f32, f32)>) -> bool {
        if escape {
            self.open = !self.open;
            self.mouse_armed = false;
            return false;
        }
        if !self.open {
            return false;
        }
        if !mouse_down {
            self.mouse_armed = true;
        } else if self.mouse_armed {
            self.mouse_armed = false;
            if let Some((x, y)) = position {
                if EXIT_YES.contains(x, y) {
                    return true;
                }
                if EXIT_NO.contains(x, y) {
                    self.open = false;
                }
            }
        }
        false
    }
}

/// The native minifb window used by the netplay frontend.
///
/// The window owns a 720x480 RGB buffer. Setup is drawn directly into that
/// buffer, while the 240x160 GBA frame is expanded to a sharp 3x image.
pub struct Ui {
    window: Window,
    buffer: Vec<u32>,
    address: String,
    port: String,
    address_cursor: usize,
    port_cursor: usize,
    focus: Focus,
    screen: Screen,
    busy: bool,
    mouse_was_down: bool,
    quit_reported: bool,
    exit_prompt: ExitPrompt,
    local_status: Option<String>,
    local_delay: u32,
    debug_visible: bool,
    debug_lines: Vec<String>,
    preferences: Preferences,
    controllers: Controllers,
    active_inputs: Vec<Binding>,
    volume_dragging: bool,
}

impl Ui {
    /// Open the one native setup/game window.
    pub fn new(root: &Path) -> Result<Self, String> {
        let preferences = Preferences::new(root.join("netplay-settings.txt"))?;
        let options = WindowOptions {
            resize: false,
            scale: Scale::X1,
            scale_mode: ScaleMode::Stretch,
            ..WindowOptions::default()
        };
        let mut window = Window::new(SETUP_TITLE, WINDOW_WIDTH, WINDOW_HEIGHT, options)
            .map_err(|error| format!("failed to open netplay window: {error}"))?;
        // The emulation loop owns frame pacing (including its 59.7275 Hz
        // GBA cadence); minifb should only present frames here.
        window.set_target_fps(0);

        Ok(Self {
            window,
            buffer: vec![BACKGROUND; WINDOW_WIDTH * WINDOW_HEIGHT],
            address: DEFAULT_ADDRESS.to_owned(),
            port: DEFAULT_PORT.to_owned(),
            address_cursor: DEFAULT_ADDRESS.len(),
            port_cursor: DEFAULT_PORT.len(),
            focus: Focus::Address,
            screen: Screen::Setup,
            busy: false,
            mouse_was_down: false,
            quit_reported: false,
            exit_prompt: ExitPrompt::default(),
            local_status: None,
            local_delay: DEFAULT_LOCAL_DELAY,
            debug_visible: false,
            debug_lines: Vec::new(),
            preferences,
            controllers: Controllers::new()?,
            active_inputs: Vec::new(),
            volume_dragging: false,
        })
    }

    /// Return the next setup command, if the active window produced one.
    pub fn poll(&mut self) -> Option<Action> {
        self.preferences.poll_save();
        if !self.window.is_open() {
            return self.report_quit();
        }

        // minifb only reports keyboard and mouse input for the active window.
        // Calling is_active also gives the backend a chance to process focus
        // changes before we inspect the input queues.
        if !self.window.is_active() {
            self.active_inputs.clear();
            self.preferences.cancel_capture(&[]);
            self.preferences.poll_inputs(&[]);
            self.volume_dragging = false;
            self.mouse_was_down = false;
            self.exit_prompt.mouse_armed = false;
            return None;
        }

        let pressed = self.window.get_keys_pressed(KeyRepeat::Yes);
        let just_pressed = self.window.get_keys_pressed(KeyRepeat::No);
        let mouse_position = self.window.get_mouse_pos(MouseMode::Discard);
        let mouse_down = self.window.get_mouse_down(MouseButton::Left);
        let clicked = mouse_down && !self.mouse_was_down;
        self.mouse_was_down = mouse_down;

        let was_open = self.exit_prompt.open;
        let escape = just_pressed.contains(&Key::Escape);
        self.active_inputs = self
            .window
            .get_keys()
            .into_iter()
            .map(Binding::Keyboard)
            .collect();
        self.active_inputs.extend(self.controllers.snapshot());
        // Escape cancels a binding without opening the exit confirmation.
        if escape && self.preferences.capture.is_some() {
            self.preferences.cancel_capture(&self.active_inputs);
            return None;
        }
        if self.screen == Screen::Game && just_pressed.contains(&Key::F1) && !was_open {
            self.debug_visible = !self.debug_visible;
            self.preferences.cancel_capture(&self.active_inputs);
            self.volume_dragging = false;
        }
        self.preferences.poll_inputs(&self.active_inputs);
        if self.exit_prompt.update(escape, mouse_down, mouse_position) {
            return self.report_quit();
        }
        if was_open || self.exit_prompt.open || escape {
            return None;
        }

        if self.screen == Screen::Game && self.debug_visible {
            self.poll_settings(clicked, mouse_down, mouse_position);
        }

        if self.screen != Screen::Setup {
            return None;
        }

        // Pasting replaces the selected field, convenient for an IP shared by
        // a friend. Clipboard access occurs only on the user's Ctrl+V action.
        if (self.window.is_key_down(Key::LeftCtrl) || self.window.is_key_down(Key::RightCtrl))
            && just_pressed.contains(&Key::V)
            && !self.busy
        {
            if let Some(value) = clipboard_text() {
                let value = value.trim();
                match self.focus {
                    Focus::Address
                        if value.len() <= 64
                            && value
                                .chars()
                                .all(|c| c.is_ascii_hexdigit() || ".:[]".contains(c)) =>
                    {
                        self.address = value.to_string();
                        self.address_cursor = value.len();
                        self.local_status = None;
                    }
                    Focus::Port
                        if value.len() <= 5 && value.chars().all(|c| c.is_ascii_digit()) =>
                    {
                        self.port = value.to_string();
                        self.port_cursor = value.len();
                        self.local_status = None;
                    }
                    _ => {
                        self.local_status =
                            Some("Paste an IP address or numeric port into its field.".into())
                    }
                }
            }
            return None;
        }

        if clicked {
            if let Some((x, y)) = mouse_position {
                if IP_FIELD.contains(x, y) {
                    self.focus = Focus::Address;
                    self.address_cursor =
                        cursor_from_mouse(&self.address, x, IP_FIELD.x, IP_FIELD.width);
                } else if PORT_FIELD.contains(x, y) {
                    self.focus = Focus::Port;
                    self.port_cursor =
                        cursor_from_mouse(&self.port, x, PORT_FIELD.x, PORT_FIELD.width);
                } else if DELAY_SELECTOR.contains(x, y) && !self.busy {
                    self.local_delay = delay_from_mouse(x);
                } else if HOST_BUTTON.contains(x, y) {
                    return self.host_action();
                } else if CONNECT_BUTTON.contains(x, y) {
                    return self.connect_action();
                }
            }
        }

        let shift_held =
            self.window.is_key_down(Key::LeftShift) || self.window.is_key_down(Key::RightShift);
        for key in pressed {
            if key == Key::Tab {
                self.focus = match self.focus {
                    Focus::Address => Focus::Port,
                    Focus::Port => Focus::Address,
                };
                continue;
            }
            if key != Key::Enter {
                self.edit_focused_field(key, shift_held);
            }
        }

        // Enter is a convenient keyboard equivalent to the Connect button.
        // It is read from the edge-triggered list so holding it cannot emit a
        // stream of duplicate connection attempts.
        if just_pressed.contains(&Key::Enter) {
            return self.connect_action();
        }

        None
    }

    /// Draw the setup screen and present the current network status.
    pub fn show_setup(&mut self, status: &str, busy: bool) -> Result<(), String> {
        if self.screen == Screen::Game {
            self.preferences.cancel_capture(&self.active_inputs);
            self.volume_dragging = false;
        }
        self.screen = Screen::Setup;
        self.busy = busy;
        self.window.set_title(SETUP_TITLE);
        self.buffer.fill(BACKGROUND);

        fill_rect(
            &mut self.buffer,
            Rect {
                x: 48,
                y: 24,
                width: 624,
                height: 432,
            },
            PANEL,
        );
        fill_rect(
            &mut self.buffer,
            Rect {
                x: 49,
                y: 25,
                width: 622,
                height: 430,
            },
            PANEL_DARK,
        );

        draw_text(
            &mut self.buffer,
            80,
            48,
            "SHINING SOUL II NETPLAY",
            ACCENT,
            2,
        );
        draw_text(
            &mut self.buffer,
            80,
            78,
            "Two-player rollback over a direct IP connection",
            MUTED_TEXT,
            1,
        );

        draw_text(&mut self.buffer, 80, 140, "IP ADDRESS", MUTED_TEXT, 1);
        draw_text(&mut self.buffer, 80, 198, "PORT", MUTED_TEXT, 1);
        draw_input_field(
            &mut self.buffer,
            IP_FIELD,
            &self.address,
            self.focus == Focus::Address,
            self.address_cursor,
        );
        draw_input_field(
            &mut self.buffer,
            PORT_FIELD,
            &self.port,
            self.focus == Focus::Port,
            self.port_cursor,
        );
        draw_text(
            &mut self.buffer,
            DELAY_SELECTOR.x,
            DELAY_SELECTOR.y.saturating_sub(10),
            "DELAY (FRAMES)",
            MUTED_TEXT,
            1,
        );
        draw_delay_selector(
            &mut self.buffer,
            DELAY_SELECTOR,
            self.local_delay,
            !self.busy,
        );

        draw_button(&mut self.buffer, HOST_BUTTON, "HOST", !self.busy, false);
        draw_button(
            &mut self.buffer,
            CONNECT_BUTTON,
            "CONNECT",
            !self.busy,
            false,
        );

        draw_status(
            &mut self.buffer,
            STATUS_PANEL,
            status,
            self.local_status.as_deref(),
            self.busy,
        );

        draw_text(
            &mut self.buffer,
            80,
            410,
            "Put one .gba in ROM. Your character auto-loads/saves in save.",
            MUTED_TEXT,
            1,
        );
        draw_text(
            &mut self.buffer,
            80,
            426,
            "Defaults: arrows, Z=A, X=B, Enter=Start, RShift=Select, C=L, V=R",
            MUTED_TEXT,
            1,
        );
        draw_text(
            &mut self.buffer,
            80,
            442,
            "F1 in game: volume / bindings. Escape: exit confirmation.",
            MUTED_TEXT,
            1,
        );

        self.present()
    }

    /// Draw one native GBA frame, expanded with nearest-neighbor 3x pixels.
    pub fn show_game(&mut self, native: &[u8], title: &str) -> Result<(), String> {
        let expected = GBA_WIDTH * GBA_HEIGHT * 2;
        if native.len() != expected {
            return Err(format!(
                "unexpected GBA framebuffer size: {} bytes (expected {expected})",
                native.len()
            ));
        }

        self.screen = Screen::Game;
        self.window
            .set_title(if title.is_empty() { SETUP_TITLE } else { title });

        for (source_index, bytes) in native.chunks_exact(2).enumerate() {
            let native_pixel = u16::from_ne_bytes([bytes[0], bytes[1]]);
            let red = expand_5_bit(u32::from(native_pixel & 0x1f));
            let green = expand_5_bit(u32::from((native_pixel >> 5) & 0x1f));
            let blue = expand_5_bit(u32::from((native_pixel >> 10) & 0x1f));
            let pixel = (red << 16) | (green << 8) | blue;

            let source_x = source_index % GBA_WIDTH;
            let source_y = source_index / GBA_WIDTH;
            let destination_x = source_x * GBA_SCALE;
            let destination_y = source_y * GBA_SCALE;
            for row in 0..GBA_SCALE {
                let start = (destination_y + row) * WINDOW_WIDTH + destination_x;
                self.buffer[start..start + GBA_SCALE].fill(pixel);
            }
        }

        self.present()
    }

    /// Return the GBA button mask while this window has focus.
    pub fn keys(&mut self) -> u32 {
        if self.exit_prompt.open || !self.window.is_open() || !self.window.is_active() {
            return 0;
        }

        self.preferences.keys(&self.active_inputs)
    }

    pub fn volume(&self) -> u8 {
        self.preferences.settings.volume
    }

    fn poll_settings(&mut self, clicked: bool, mouse_down: bool, position: Option<(f32, f32)>) {
        if !mouse_down {
            self.volume_dragging = false;
        }
        if let Some((x, y)) = position {
            if clicked && VOLUME_SLIDER.contains(x, y) {
                self.volume_dragging = true;
            }
            if self.volume_dragging {
                self.preferences.set_volume(volume_from_mouse(x));
            }
            if clicked {
                for action in 0..ACTION_NAMES.len() {
                    if binding_rect(action).contains(x, y) {
                        self.preferences.begin_capture(action, &self.active_inputs);
                        break;
                    }
                }
            }
        }
    }

    /// Whether the native window still exists.
    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    /// Return the local input delay selected on the setup screen.
    pub fn selected_delay(&self) -> u32 {
        self.local_delay
    }

    /// Replace the lines shown by the optional in-game diagnostics overlay.
    pub fn set_debug_lines(&mut self, lines: Vec<String>) {
        self.debug_lines = lines;
    }

    /// Capture a representative in-game diagnostics overlay for build-time
    /// visual inspection without changing the current screen or exit prompt.
    pub fn write_debug_preview(&mut self, path: &Path) -> Result<(), String> {
        let previous_buffer = self.buffer.clone();
        let previous_screen = self.screen;
        let previous_debug_visible = self.debug_visible;
        let previous_debug_lines = self.debug_lines.clone();
        let previous_exit_open = self.exit_prompt.open;
        let previous_mouse_armed = self.exit_prompt.mouse_armed;

        self.screen = Screen::Game;
        self.debug_visible = true;
        self.debug_lines = vec![
            "NETPLAY STATS - F1 hides".to_owned(),
            "Ping RTT: 200.0 ms".to_owned(),
            "Game FPS (500ms): 59.7".to_owned(),
            "Delay: 4 frames".to_owned(),
            "Rollbacks total: 3".to_owned(),
            "Rollbacks last 60s: 1".to_owned(),
            "Depth last / max: 4 / 8".to_owned(),
            "Prediction: 3 frames".to_owned(),
            "Input queue: 6 / 10".to_owned(),
            "Waiting for inputs: No".to_owned(),
            "Synced frame: 1200".to_owned(),
            "Recommended delay: 4 frames (high lateness)".to_owned(),
        ];
        // The exit modal must not obscure the preview overlay, even when a
        // caller captures a preview while the real prompt is open.
        self.exit_prompt.open = false;
        self.exit_prompt.mouse_armed = false;

        let result = (|| {
            let frame = debug_preview_frame();
            self.show_game(&frame, "SS2 | Debug preview")?;
            self.write_preview(path)
        })();

        self.buffer = previous_buffer;
        self.screen = previous_screen;
        self.debug_visible = previous_debug_visible;
        self.debug_lines = previous_debug_lines;
        self.exit_prompt.open = previous_exit_open;
        self.exit_prompt.mouse_armed = previous_mouse_armed;
        result
    }

    /// Capture the exact rendered pixels for build-time visual inspection.
    pub fn write_exit_preview(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.exit_prompt.open = true;
        self.present()?;
        self.write_preview(path)
    }

    /// Capture the exact rendered pixels for build-time visual inspection.
    pub fn write_preview(&self, path: &std::path::Path) -> Result<(), String> {
        use std::io::Write;
        let bytes = WINDOW_WIDTH * WINDOW_HEIGHT * 3;
        let mut header = vec![0u8; 54];
        header[0..2].copy_from_slice(b"BM");
        header[2..6].copy_from_slice(&((54 + bytes) as u32).to_le_bytes());
        header[10..14].copy_from_slice(&54u32.to_le_bytes());
        header[14..18].copy_from_slice(&40u32.to_le_bytes());
        header[18..22].copy_from_slice(&(WINDOW_WIDTH as u32).to_le_bytes());
        header[22..26].copy_from_slice(&(WINDOW_HEIGHT as u32).to_le_bytes());
        header[26..28].copy_from_slice(&1u16.to_le_bytes());
        header[28..30].copy_from_slice(&24u16.to_le_bytes());
        let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        file.write_all(&header).map_err(|e| e.to_string())?;
        for row in self.buffer.chunks_exact(WINDOW_WIDTH).rev() {
            for pixel in row {
                file.write_all(&[*pixel as u8, (*pixel >> 8) as u8, (*pixel >> 16) as u8])
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    fn present(&mut self) -> Result<(), String> {
        if self.screen == Screen::Game && self.debug_visible {
            draw_debug_overlay(&mut self.buffer, &self.debug_lines);
            draw_settings_overlay(&mut self.buffer, &self.preferences);
        }
        if self.exit_prompt.open {
            let panel = Rect {
                x: 115,
                y: 152,
                width: 490,
                height: 184,
            };
            fill_rect(&mut self.buffer, panel, PANEL);
            stroke_rect(&mut self.buffer, panel, 0x7596b8);
            draw_text(
                &mut self.buffer,
                150,
                179,
                "Are you sure you want to exit?",
                0xffffff,
                1,
            );
            draw_text(
                &mut self.buffer,
                150,
                205,
                "The session keeps running while this is open.",
                0xb9c8d8,
                1,
            );
            draw_text(
                &mut self.buffer,
                150,
                226,
                "Click Yes to exit. Escape cancels.",
                0xb9c8d8,
                1,
            );
            draw_button(&mut self.buffer, EXIT_NO, "Keep playing", true, false);
            draw_button(&mut self.buffer, EXIT_YES, "Yes, exit", true, false);
        }
        self.window
            .update_with_buffer(&self.buffer, WINDOW_WIDTH, WINDOW_HEIGHT)
            .map_err(|error| format!("failed to present netplay window: {error}"))
    }

    fn report_quit(&mut self) -> Option<Action> {
        if self.quit_reported {
            None
        } else {
            self.quit_reported = true;
            Some(Action::Quit)
        }
    }

    fn host_action(&mut self) -> Option<Action> {
        if self.busy {
            return None;
        }
        match parse_port(&self.port) {
            Ok(port) => {
                self.local_status = None;
                Some(Action::Host(port))
            }
            Err(error) => {
                self.local_status = Some(error);
                None
            }
        }
    }

    fn connect_action(&mut self) -> Option<Action> {
        if self.busy {
            return None;
        }

        let port = match parse_port(&self.port) {
            Ok(port) => port,
            Err(error) => {
                self.local_status = Some(error);
                return None;
            }
        };
        let address = self.address.trim();
        let address = address
            .strip_prefix('[')
            .and_then(|address| address.strip_suffix(']'))
            .unwrap_or(address);
        match IpAddr::from_str(address) {
            Ok(ip) => {
                self.local_status = None;
                Some(Action::Connect(SocketAddr::new(ip, port)))
            }
            Err(_) => {
                self.local_status = Some(format!(
                    "Invalid IP address: {}. Use numeric IPv4 or IPv6.",
                    self.address.trim()
                ));
                None
            }
        }
    }

    fn edit_focused_field(&mut self, key: Key, shift_held: bool) {
        match self.focus {
            Focus::Address => edit_text(
                &mut self.address,
                &mut self.address_cursor,
                key,
                address_character(key, shift_held),
                63,
            ),
            Focus::Port => edit_text(
                &mut self.port,
                &mut self.port_cursor,
                key,
                port_character(key),
                5,
            ),
        }
    }
}

#[cfg(test)]
mod exit_prompt_tests {
    use super::*;

    #[test]
    fn escape_cancels_and_keyboard_alone_never_confirms() {
        let mut prompt = ExitPrompt::default();
        assert!(!prompt.update(true, false, None));
        assert!(prompt.open);
        // Keyboard keys other than Escape have no route to confirmation.
        for _ in 0..10 {
            assert!(!prompt.update(false, false, Some((400.0, 280.0))));
        }
        assert!(!prompt.update(true, false, None));
        assert!(!prompt.open);
    }

    #[test]
    fn confirmation_requires_release_then_click_inside_yes() {
        let mut prompt = ExitPrompt::default();
        let yes = Some((400.0, 280.0));
        assert!(!prompt.update(true, true, yes));
        assert!(!prompt.update(false, true, yes));
        assert!(!prompt.update(false, false, yes));
        // Clicking elsewhere then dragging onto Yes must not confirm either.
        assert!(!prompt.update(false, true, Some((0.0, 0.0))));
        assert!(!prompt.update(false, true, yes));
        assert!(!prompt.update(false, false, yes));
        assert!(prompt.update(false, true, yes));
    }

    #[test]
    fn keep_playing_click_closes_prompt() {
        let mut prompt = ExitPrompt::default();
        prompt.update(true, false, None);
        prompt.update(false, false, None);
        assert!(!prompt.update(false, true, Some((170.0, 280.0))));
        assert!(!prompt.open);
    }
}

fn parse_port(value: &str) -> Result<u16, String> {
    let value = value.trim();
    match value.parse::<u16>() {
        Ok(port) if port != 0 => Ok(port),
        Ok(_) => Err("Port must be between 1 and 65535.".to_owned()),
        Err(_) => Err(format!("Invalid port: {value}. Use 1-65535.")),
    }
}

fn edit_text(
    text: &mut String,
    cursor: &mut usize,
    key: Key,
    character: Option<char>,
    max_len: usize,
) {
    *cursor = (*cursor).min(text.len());
    match key {
        Key::Backspace => {
            if *cursor > 0 {
                text.remove(*cursor - 1);
                *cursor -= 1;
            }
        }
        Key::Delete => {
            if *cursor < text.len() {
                text.remove(*cursor);
            }
        }
        Key::Left => *cursor = cursor.saturating_sub(1),
        Key::Right => *cursor = (*cursor + 1).min(text.len()),
        Key::Home => *cursor = 0,
        Key::End => *cursor = text.len(),
        _ => {
            if let Some(character) = character {
                if text.len() < max_len {
                    text.insert(*cursor, character);
                    *cursor += character.len_utf8();
                }
            }
        }
    }
}

fn address_character(key: Key, _shift_held: bool) -> Option<char> {
    match key {
        Key::Key0 | Key::NumPad0 => Some('0'),
        Key::Key1 | Key::NumPad1 => Some('1'),
        Key::Key2 | Key::NumPad2 => Some('2'),
        Key::Key3 | Key::NumPad3 => Some('3'),
        Key::Key4 | Key::NumPad4 => Some('4'),
        Key::Key5 | Key::NumPad5 => Some('5'),
        Key::Key6 | Key::NumPad6 => Some('6'),
        Key::Key7 | Key::NumPad7 => Some('7'),
        Key::Key8 | Key::NumPad8 => Some('8'),
        Key::Key9 | Key::NumPad9 => Some('9'),
        Key::A => Some('a'),
        Key::B => Some('b'),
        Key::C => Some('c'),
        Key::D => Some('d'),
        Key::E => Some('e'),
        Key::F => Some('f'),
        Key::Period | Key::NumPadDot => Some('.'),
        // minifb reports the physical semicolon key for both ';' and ':'.
        // A colon is the useful form in an IPv6 address field.
        Key::Semicolon => Some(':'),
        Key::LeftBracket => Some('['),
        Key::RightBracket => Some(']'),
        _ => None,
    }
}

fn port_character(key: Key) -> Option<char> {
    match key {
        Key::Key0 | Key::NumPad0 => Some('0'),
        Key::Key1 | Key::NumPad1 => Some('1'),
        Key::Key2 | Key::NumPad2 => Some('2'),
        Key::Key3 | Key::NumPad3 => Some('3'),
        Key::Key4 | Key::NumPad4 => Some('4'),
        Key::Key5 | Key::NumPad5 => Some('5'),
        Key::Key6 | Key::NumPad6 => Some('6'),
        Key::Key7 | Key::NumPad7 => Some('7'),
        Key::Key8 | Key::NumPad8 => Some('8'),
        Key::Key9 | Key::NumPad9 => Some('9'),
        _ => None,
    }
}

fn cursor_from_mouse(text: &str, mouse_x: f32, field_x: usize, field_width: usize) -> usize {
    let relative = (mouse_x - field_x as f32 - 10.0).max(0.0);
    let position = (relative / 8.0).round() as usize;
    position
        .min(text.len())
        .min(field_width.saturating_sub(16) / 8)
}

fn expand_5_bit(value: u32) -> u32 {
    (value << 3) | (value >> 2)
}

fn debug_preview_frame() -> Vec<u8> {
    let mut frame = vec![0u8; GBA_WIDTH * GBA_HEIGHT * 2];
    for y in 0..GBA_HEIGHT {
        for x in 0..GBA_WIDTH {
            let tile = ((x / 24) + (y / 16)) % 2;
            let red = ((x * 31) / GBA_WIDTH).min(31) as u16;
            let green = ((y * 31) / GBA_HEIGHT).min(31) as u16;
            let blue = if tile == 0 { 10 } else { 20 };
            let pixel = red | (green << 5) | (blue << 10);
            let offset = (y * GBA_WIDTH + x) * 2;
            frame[offset..offset + 2].copy_from_slice(&pixel.to_ne_bytes());
        }
    }
    frame
}

fn fill_rect(buffer: &mut [u32], rect: Rect, color: u32) {
    let max_y = (rect.y + rect.height).min(WINDOW_HEIGHT);
    let max_x = (rect.x + rect.width).min(WINDOW_WIDTH);
    for y in rect.y.min(WINDOW_HEIGHT)..max_y {
        let row = y * WINDOW_WIDTH;
        buffer[row + rect.x.min(WINDOW_WIDTH)..row + max_x].fill(color);
    }
}

fn stroke_rect(buffer: &mut [u32], rect: Rect, color: u32) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    fill_rect(
        buffer,
        Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: 1,
        },
        color,
    );
    fill_rect(
        buffer,
        Rect {
            x: rect.x,
            y: rect.y + rect.height.saturating_sub(1),
            width: rect.width,
            height: 1,
        },
        color,
    );
    fill_rect(
        buffer,
        Rect {
            x: rect.x,
            y: rect.y,
            width: 1,
            height: rect.height,
        },
        color,
    );
    fill_rect(
        buffer,
        Rect {
            x: rect.x + rect.width.saturating_sub(1),
            y: rect.y,
            width: 1,
            height: rect.height,
        },
        color,
    );
}

fn draw_input_field(buffer: &mut [u32], rect: Rect, value: &str, focused: bool, cursor: usize) {
    fill_rect(buffer, rect, if focused { FIELD_FOCUS } else { FIELD });
    stroke_rect(buffer, rect, if focused { ACCENT } else { ACCENT_DARK });
    draw_text(buffer, rect.x + 10, rect.y + 11, value, TEXT, 1);
    if focused {
        let cursor_x = rect.x + 10 + cursor.min(rect.width.saturating_sub(20) / 8) * 8;
        fill_rect(
            buffer,
            Rect {
                x: cursor_x,
                y: rect.y + 8,
                width: 1,
                height: rect.height - 16,
            },
            ACCENT,
        );
    }
}

fn draw_button(buffer: &mut [u32], rect: Rect, label: &str, enabled: bool, hovered: bool) {
    let fill = if !enabled {
        BUTTON_DISABLED
    } else if hovered {
        BUTTON_HOVER
    } else {
        BUTTON
    };
    fill_rect(buffer, rect, fill);
    stroke_rect(buffer, rect, if enabled { ACCENT } else { ACCENT_DARK });
    let text_width = label.len() * 8;
    let text_x = rect.x + rect.width.saturating_sub(text_width) / 2;
    let text_color = if enabled { TEXT } else { MUTED_TEXT };
    draw_text(buffer, text_x, rect.y + 20, label, text_color, 1);
}

fn draw_delay_selector(buffer: &mut [u32], rect: Rect, selected: u32, enabled: bool) {
    for (index, delay) in (1..=4).enumerate() {
        let segment = delay_segment_rect(rect, index);
        let fill = if !enabled {
            if delay == selected {
                BUTTON_SELECTED_DISABLED
            } else {
                BUTTON_DISABLED
            }
        } else if delay == selected {
            ACCENT_DARK
        } else {
            BUTTON
        };
        fill_rect(buffer, segment, fill);
        stroke_rect(buffer, segment, if enabled { ACCENT } else { ACCENT_DARK });

        let label = delay.to_string();
        let text_width = label.len() * 8;
        let text_x = segment.x + segment.width.saturating_sub(text_width) / 2;
        let text_color = if enabled { TEXT } else { MUTED_TEXT };
        draw_text(buffer, text_x, segment.y + 15, &label, text_color, 1);
    }
}

fn delay_segment_rect(rect: Rect, index: usize) -> Rect {
    let segment_width = rect.width / 4;
    let x = rect.x + segment_width * index;
    let width = if index == 3 {
        rect.width - segment_width * 3
    } else {
        segment_width
    };
    Rect {
        x,
        y: rect.y,
        width,
        height: rect.height,
    }
}

fn delay_from_mouse(x: f32) -> u32 {
    let segment_width = DELAY_SELECTOR.width as f32 / 4.0;
    let index = ((x - DELAY_SELECTOR.x as f32) / segment_width)
        .floor()
        .clamp(0.0, 3.0) as u32;
    index + 1
}

fn draw_settings_overlay(buffer: &mut [u32], preferences: &Preferences) {
    fill_rect(buffer, SETTINGS_PANEL, PANEL);
    stroke_rect(buffer, SETTINGS_PANEL, ACCENT_DARK);
    draw_text(buffer, 24, 160, "LOCAL SETTINGS", ACCENT, 1);
    draw_text(
        buffer,
        392,
        160,
        "Game keeps running - F1 hides",
        MUTED_TEXT,
        1,
    );
    draw_text(buffer, 24, 193, "Volume", TEXT, 1);
    fill_rect(buffer, VOLUME_SLIDER, FIELD);
    stroke_rect(buffer, VOLUME_SLIDER, ACCENT_DARK);
    let volume = preferences.settings.volume;
    let offset = (VOLUME_SLIDER.width - 1) * usize::from(volume) / 100;
    fill_rect(
        buffer,
        Rect {
            x: VOLUME_SLIDER.x,
            y: 194,
            width: offset,
            height: 6,
        },
        ACCENT_DARK,
    );
    fill_rect(
        buffer,
        Rect {
            x: VOLUME_SLIDER.x + offset.saturating_sub(3),
            y: 187,
            width: 4,
            height: 20,
        },
        ACCENT,
    );
    draw_text(buffer, 650, 193, &format!("{volume}%"), TEXT, 1);
    draw_text(
        buffer,
        24,
        223,
        "INPUT MAPPER - click a GBA button, then press an input",
        ACCENT,
        1,
    );
    for (action, name) in ACTION_NAMES.iter().enumerate() {
        let rect = binding_rect(action);
        let capturing = preferences.capture == Some(action);
        fill_rect(buffer, rect, if capturing { FIELD_FOCUS } else { FIELD });
        stroke_rect(buffer, rect, if capturing { ACCENT } else { ACCENT_DARK });
        draw_text(buffer, rect.x + 8, rect.y + 8, name, TEXT, 1);
        let binding = if capturing {
            "Press input...".into()
        } else {
            preferences.settings.bindings[action].to_string()
        };
        let label: String = binding.chars().take(27).collect();
        draw_text(
            buffer,
            rect.x + 104,
            rect.y + 8,
            &label,
            if capturing { ACCENT } else { MUTED_TEXT },
            1,
        );
    }
    let hint = if preferences.capture.is_some() {
        "Press a NEW key / pad button / axis. Escape cancels."
    } else {
        "Keyboard or XInput pad; sticks + triggers supported."
    };
    draw_text(buffer, 24, 395, hint, TEXT, 1);
    draw_text(
        buffer,
        24,
        411,
        "F1 / Escape reserved. Mapping replaces that button's input.",
        MUTED_TEXT,
        1,
    );
    let color = if preferences.save_failed {
        ERROR
    } else {
        MUTED_TEXT
    };
    for (row, line) in wrap_text(&preferences.status, 82)
        .iter()
        .take(3)
        .enumerate()
    {
        draw_text(buffer, 24, 432 + row * 11, line, color, 1);
    }
}

fn draw_debug_overlay(buffer: &mut [u32], lines: &[String]) {
    let mut display_lines = Vec::new();
    for line in lines {
        for part in line.split('\n') {
            let clipped = part.chars().take(DEBUG_MAX_COLUMNS).collect::<String>();
            display_lines.push(if clipped.is_empty() {
                " ".to_owned()
            } else {
                clipped
            });
            if display_lines.len() == DEBUG_MAX_LINES {
                break;
            }
        }
        if display_lines.len() == DEBUG_MAX_LINES {
            break;
        }
    }
    if display_lines.is_empty() {
        display_lines.push("DEBUG ON".to_owned());
    }

    let max_chars = display_lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
        .max(8);
    let rect = Rect {
        x: 8,
        y: 8,
        width: (max_chars + 2) * 8,
        height: display_lines.len() * 10 + 8,
    };
    fill_rect(buffer, rect, DEBUG_BACKGROUND);
    stroke_rect(buffer, rect, DEBUG_TEXT);
    for (index, line) in display_lines.iter().enumerate() {
        draw_text(
            buffer,
            rect.x + 8,
            rect.y + 4 + index * 10,
            line,
            DEBUG_TEXT,
            1,
        );
    }
}

fn draw_status(
    buffer: &mut [u32],
    rect: Rect,
    status: &str,
    local_status: Option<&str>,
    busy: bool,
) {
    fill_rect(buffer, rect, PANEL_DARK);
    stroke_rect(buffer, rect, ACCENT_DARK);
    draw_text(buffer, rect.x + 10, rect.y + 8, "STATUS", ACCENT, 1);
    if busy {
        draw_text(
            buffer,
            rect.x + rect.width - 72,
            rect.y + 8,
            "BUSY",
            ACCENT,
            1,
        );
    }

    let mut line_y = rect.y + 26;
    let max_y = rect.y + rect.height.saturating_sub(8);
    let display = local_status.unwrap_or(status);
    let columns = (rect.width - 20) / 8;
    let wrapped = wrap_text(display, columns);
    for line in &wrapped {
        if line_y + 8 > max_y {
            break;
        }
        let color = if local_status.is_some() {
            ERROR
        } else {
            MUTED_TEXT
        };
        draw_text(buffer, rect.x + 10, line_y, line, color, 1);
        line_y += 12;
    }
    if line_y == rect.y + 26 {
        draw_text(buffer, rect.x + 10, line_y, "Ready.", MUTED_TEXT, 1);
    }
}

fn wrap_text(text: &str, columns: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        for part in word.as_bytes().chunks(columns) {
            let part = String::from_utf8_lossy(part);
            if !line.is_empty() && line.len() + 1 + part.len() > columns {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&part);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(windows)]
fn clipboard_text() -> Option<String> {
    use std::ffi::c_void;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenClipboard(window: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut c_void;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalLock(memory: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(memory: *mut c_void) -> i32;
        fn GlobalSize(memory: *mut c_void) -> usize;
    }
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let handle = GetClipboardData(13); // CF_UNICODETEXT
        let data = if handle.is_null() {
            std::ptr::null_mut()
        } else {
            GlobalLock(handle)
        };
        let result = if data.is_null() {
            None
        } else {
            let text =
                std::slice::from_raw_parts(data.cast::<u16>(), (GlobalSize(handle) / 2).min(256));
            Some(String::from_utf16_lossy(
                &text[..text.iter().position(|v| *v == 0).unwrap_or(text.len())],
            ))
        };
        if !data.is_null() {
            GlobalUnlock(handle);
        }
        CloseClipboard();
        result
    }
}
#[cfg(not(windows))]
fn clipboard_text() -> Option<String> {
    None
}

fn draw_text(buffer: &mut [u32], x: usize, y: usize, text: &str, color: u32, scale: usize) {
    if scale == 0 {
        return;
    }
    let mut pen_x = x;
    let mut pen_y = y;
    for character in text.chars() {
        if character == '\n' {
            pen_x = x;
            pen_y += 8 * scale + 2;
            continue;
        }

        if let Some(glyph) = BASIC_FONTS.get(character).or_else(|| BASIC_FONTS.get('?')) {
            for (row, bits) in glyph.iter().enumerate() {
                for column in 0..8 {
                    if bits & (1 << column) == 0 {
                        continue;
                    }
                    fill_rect(
                        buffer,
                        Rect {
                            x: pen_x + column * scale,
                            y: pen_y + row * scale,
                            width: scale,
                            height: scale,
                        },
                        color,
                    );
                }
            }
        }
        pen_x += 8 * scale;
    }
}
