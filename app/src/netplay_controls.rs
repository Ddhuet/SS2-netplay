//! Configurable local controls for the netplay frontend.
//!
//! The game loop deals in the ten bits exposed by `mgba::input::keys`.  This
//! module keeps the device specific part of input handling at the edge of the
//! application: a keyboard or an XInput device is represented by a
//! [`Binding`], and [`Settings::mask`] translates the currently active
//! bindings into a GBA key mask.

use minifb::Key;
use std::fmt;
use std::fs;
use std::path::Path;

/// Names and order of the ten GBA actions used by Shining Soul II.
pub const ACTION_NAMES: [&str; 10] = [
    "A", "B", "Select", "Start", "Right", "Left", "Up", "Down", "R", "L",
];

/// XInput button bits from `XINPUT_GAMEPAD`.
pub const XINPUT_GAMEPAD_DPAD_UP: u16 = 0x0001;
pub const XINPUT_GAMEPAD_DPAD_DOWN: u16 = 0x0002;
pub const XINPUT_GAMEPAD_DPAD_LEFT: u16 = 0x0004;
pub const XINPUT_GAMEPAD_DPAD_RIGHT: u16 = 0x0008;
pub const XINPUT_GAMEPAD_START: u16 = 0x0010;
pub const XINPUT_GAMEPAD_BACK: u16 = 0x0020;
pub const XINPUT_GAMEPAD_LEFT_THUMB: u16 = 0x0040;
pub const XINPUT_GAMEPAD_RIGHT_THUMB: u16 = 0x0080;
pub const XINPUT_GAMEPAD_LEFT_SHOULDER: u16 = 0x0100;
pub const XINPUT_GAMEPAD_RIGHT_SHOULDER: u16 = 0x0200;
pub const XINPUT_GAMEPAD_A: u16 = 0x1000;
pub const XINPUT_GAMEPAD_B: u16 = 0x2000;
pub const XINPUT_GAMEPAD_X: u16 = 0x4000;
pub const XINPUT_GAMEPAD_Y: u16 = 0x8000;

/// A button reported by an XInput controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum XInputButton {
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    Start,
    Back,
    LeftThumb,
    RightThumb,
    LeftShoulder,
    RightShoulder,
    A,
    B,
    X,
    Y,
}

impl XInputButton {
    fn config_name(self) -> &'static str {
        match self {
            Self::DPadUp => "dpad_up",
            Self::DPadDown => "dpad_down",
            Self::DPadLeft => "dpad_left",
            Self::DPadRight => "dpad_right",
            Self::Start => "start",
            Self::Back => "back",
            Self::LeftThumb => "left_thumb",
            Self::RightThumb => "right_thumb",
            Self::LeftShoulder => "left_shoulder",
            Self::RightShoulder => "right_shoulder",
            Self::A => "a",
            Self::B => "b",
            Self::X => "x",
            Self::Y => "y",
        }
    }

    fn parse_config_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "dpad_up" | "dpadup" | "up" => Some(Self::DPadUp),
            "dpad_down" | "dpaddown" | "down" => Some(Self::DPadDown),
            "dpad_left" | "dpadleft" => Some(Self::DPadLeft),
            "dpad_right" | "dpadright" => Some(Self::DPadRight),
            "start" => Some(Self::Start),
            "back" | "select" => Some(Self::Back),
            "left_thumb" | "leftthumb" | "left_stick_button" => Some(Self::LeftThumb),
            "right_thumb" | "rightthumb" | "right_stick_button" => Some(Self::RightThumb),
            "left_shoulder" | "leftshoulder" | "lb" => Some(Self::LeftShoulder),
            "right_shoulder" | "rightshoulder" | "rb" => Some(Self::RightShoulder),
            "a" => Some(Self::A),
            "b" => Some(Self::B),
            "x" => Some(Self::X),
            "y" => Some(Self::Y),
            _ => None,
        }
    }
}

impl fmt::Display for XInputButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::DPadUp => "D-pad Up",
            Self::DPadDown => "D-pad Down",
            Self::DPadLeft => "D-pad Left",
            Self::DPadRight => "D-pad Right",
            Self::Start => "Start",
            Self::Back => "Back",
            Self::LeftThumb => "Left Stick",
            Self::RightThumb => "Right Stick",
            Self::LeftShoulder => "Left Shoulder",
            Self::RightShoulder => "Right Shoulder",
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
        };
        f.write_str(name)
    }
}

/// A signed XInput stick axis.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum XInputAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
}

impl XInputAxis {
    fn config_name(self) -> &'static str {
        match self {
            Self::LeftStickX => "left_x",
            Self::LeftStickY => "left_y",
            Self::RightStickX => "right_x",
            Self::RightStickY => "right_y",
        }
    }

    fn parse_config_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "left_x" | "left_stick_x" | "leftx" => Some(Self::LeftStickX),
            "left_y" | "left_stick_y" | "lefty" => Some(Self::LeftStickY),
            "right_x" | "right_stick_x" | "rightx" => Some(Self::RightStickX),
            "right_y" | "right_stick_y" | "righty" => Some(Self::RightStickY),
            _ => None,
        }
    }

    fn value(self, state: XInputState) -> i16 {
        match self {
            Self::LeftStickX => state.left_stick_x,
            Self::LeftStickY => state.left_stick_y,
            Self::RightStickX => state.right_stick_x,
            Self::RightStickY => state.right_stick_y,
        }
    }
}

impl fmt::Display for XInputAxis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::LeftStickX => "Left Stick X",
            Self::LeftStickY => "Left Stick Y",
            Self::RightStickX => "Right Stick X",
            Self::RightStickY => "Right Stick Y",
        };
        f.write_str(name)
    }
}

/// Which side of a signed stick axis is active.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AxisDirection {
    Negative,
    Positive,
}

impl AxisDirection {
    fn config_name(self) -> &'static str {
        match self {
            Self::Negative => "negative",
            Self::Positive => "positive",
        }
    }

    fn parse_config_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "negative" | "minus" | "-" => Some(Self::Negative),
            "positive" | "plus" | "+" => Some(Self::Positive),
            _ => None,
        }
    }
}

impl fmt::Display for AxisDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Negative => "-",
            Self::Positive => "+",
        })
    }
}

/// An XInput trigger. Triggers are unsigned and become active past the
/// trigger deadzone.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum XInputTrigger {
    Left,
    Right,
}

impl XInputTrigger {
    fn config_name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    fn parse_config_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "left" | "lt" => Some(Self::Left),
            "right" | "rt" => Some(Self::Right),
            _ => None,
        }
    }
}

impl fmt::Display for XInputTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Left => "Left Trigger",
            Self::Right => "Right Trigger",
        })
    }
}

/// A physical input that may be assigned to one game action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Binding {
    /// A key from minifb's cross-platform keyboard enum.
    Keyboard(Key),
    /// An XInput button. `controller` is the zero-based XInput slot (0..4).
    XInputButton {
        controller: u8,
        button: XInputButton,
    },
    /// A signed XInput stick direction.
    XInputAxis {
        controller: u8,
        axis: XInputAxis,
        direction: AxisDirection,
    },
    /// An XInput trigger.
    XInputTrigger {
        controller: u8,
        trigger: XInputTrigger,
    },
}

impl Binding {
    /// Return the stable text representation used by the settings file.
    pub fn config_name(self) -> String {
        match self {
            Self::Keyboard(key) => format!("keyboard:{}", key_name(key)),
            Self::XInputButton { controller, button } => {
                format!("xinput:{}:button:{}", controller + 1, button.config_name())
            }
            Self::XInputAxis {
                controller,
                axis,
                direction,
            } => format!(
                "xinput:{}:axis:{}:{}",
                controller + 1,
                axis.config_name(),
                direction.config_name()
            ),
            Self::XInputTrigger {
                controller,
                trigger,
            } => format!(
                "xinput:{}:trigger:{}",
                controller + 1,
                trigger.config_name()
            ),
        }
    }

    /// Parse the stable text representation, accepting a few readable aliases
    /// so hand-edited settings remain pleasant to use.
    pub fn parse_config_name(text: &str) -> Option<Self> {
        let text = text.trim();
        if let Some(key) = text
            .strip_prefix("keyboard:")
            .or_else(|| text.strip_prefix("key:"))
            .and_then(parse_key_name)
        {
            return Some(Self::Keyboard(key));
        }

        // Also accept a bare key name. This makes a small hand-written config
        // such as `A=Z` unsurprising while saved files remain unambiguous.
        if !text.contains(':') {
            if let Some(key) = parse_key_name(text) {
                return Some(Self::Keyboard(key));
            }
        }

        let mut fields = text.split(':');
        let prefix = fields.next()?.trim().to_ascii_lowercase();
        if prefix != "xinput" && prefix != "x" {
            return None;
        }
        let controller_text = fields.next()?.trim();
        let controller_number = controller_text.parse::<u8>().ok()?;
        // Saved files use human-facing slot numbers 1..4. Accepting 0 is
        // useful for callers who naturally write the native zero-based slot.
        let controller = if controller_number == 0 {
            0
        } else if (1..=4).contains(&controller_number) {
            controller_number - 1
        } else {
            return None;
        };
        let kind = fields.next()?.trim().to_ascii_lowercase();
        match kind.as_str() {
            "button" | "buttons" => {
                let button = XInputButton::parse_config_name(fields.next()?.trim())?;
                if fields.next().is_some() {
                    return None;
                }
                Some(Self::XInputButton { controller, button })
            }
            "axis" | "stick" => {
                let axis = XInputAxis::parse_config_name(fields.next()?.trim())?;
                let direction = AxisDirection::parse_config_name(fields.next()?.trim())?;
                if fields.next().is_some() {
                    return None;
                }
                Some(Self::XInputAxis {
                    controller,
                    axis,
                    direction,
                })
            }
            "trigger" | "triggers" => {
                let trigger = XInputTrigger::parse_config_name(fields.next()?.trim())?;
                if fields.next().is_some() {
                    return None;
                }
                Some(Self::XInputTrigger {
                    controller,
                    trigger,
                })
            }
            _ => None,
        }
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Keyboard(key) => f.write_str(key_name(key)),
            Self::XInputButton { controller, button } => {
                write!(f, "XInput {} {button}", controller + 1)
            }
            Self::XInputAxis {
                controller,
                axis,
                direction,
            } => write!(f, "XInput {} {axis} {direction}", controller + 1),
            Self::XInputTrigger {
                controller,
                trigger,
            } => write!(f, "XInput {} {trigger}", controller + 1),
        }
    }
}

/// The local controls and output volume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    /// Master output volume, in percent.
    pub volume: u8,
    /// Bindings in [`ACTION_NAMES`] order.
    pub bindings: [Binding; 10],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 100,
            bindings: [
                Binding::Keyboard(Key::Z),
                Binding::Keyboard(Key::X),
                Binding::Keyboard(Key::RightShift),
                Binding::Keyboard(Key::Enter),
                Binding::Keyboard(Key::Right),
                Binding::Keyboard(Key::Left),
                Binding::Keyboard(Key::Up),
                Binding::Keyboard(Key::Down),
                Binding::Keyboard(Key::V),
                Binding::Keyboard(Key::C),
            ],
        }
    }
}

impl Settings {
    /// Load settings from a small versioned text file.
    ///
    /// A missing file produces defaults without a warning; callers can then
    /// save those defaults. Malformed input is left untouched on disk and is
    /// reported as a warning while valid fields are retained.
    pub fn load<P: AsRef<Path>>(path: P) -> (Self, Option<String>) {
        let path = path.as_ref();
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), None)
            }
            Err(error) => {
                return (
                    Self::default(),
                    Some(format!("could not read {}: {error}", path.display())),
                )
            }
        };

        let mut settings = Self::default();
        let mut warnings = Vec::new();
        let mut version_seen = false;

        for (line_index, line) in text.lines().enumerate() {
            let line_number = line_index + 1;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            if let Some(version) = line.strip_prefix("SS2_SETTINGS ") {
                version_seen = true;
                if version.trim() != "1" {
                    warnings.push(format!("line {line_number}: unsupported settings version"));
                }
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                warnings.push(format!("line {line_number}: expected key=value"));
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            if key.eq_ignore_ascii_case("version") {
                version_seen = true;
                if value != "1" {
                    warnings.push(format!("line {line_number}: unsupported settings version"));
                }
            } else if key.eq_ignore_ascii_case("volume") {
                match value.parse::<u16>() {
                    Ok(volume) if volume <= 100 => settings.volume = volume as u8,
                    Ok(_) => warnings.push(format!(
                        "line {line_number}: volume must be between 0 and 100"
                    )),
                    Err(_) => warnings.push(format!("line {line_number}: invalid volume")),
                }
            } else {
                let action = key
                    .strip_prefix("binding.")
                    .or_else(|| key.strip_prefix("bind."))
                    .unwrap_or(key);
                let Some(action_index) = ACTION_NAMES
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(action.trim()))
                else {
                    warnings.push(format!("line {line_number}: unknown setting {key}"));
                    continue;
                };
                match Binding::parse_config_name(value) {
                    Some(binding) => settings.bindings[action_index] = binding,
                    None => warnings.push(format!("line {line_number}: invalid binding")),
                }
            }
        }

        if !version_seen {
            warnings.push("settings file has no version header".to_owned());
        }

        let warning = if warnings.is_empty() {
            None
        } else {
            Some(format!("{}: {}", path.display(), warnings.join("; ")))
        };
        (settings, warning)
    }

    /// Persist settings in a deterministic, hand-editable format.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let path = path.as_ref();
        let mut text = String::from(
            "# Shining Soul II rollback settings\n# Binding values may be keyboard or XInput inputs.\nSS2_SETTINGS 1\n",
        );
        text.push_str(&format!("volume={}\n", self.volume.min(100)));
        for (action, binding) in ACTION_NAMES.iter().zip(self.bindings.iter()) {
            text.push_str(&format!("binding.{action}={}\n", binding.config_name()));
        }
        // Replace only after the complete new file has reached disk. A failed
        // write leaves the previously saved preferences usable.
        use std::io::Write;
        let temporary = path.with_extension("txt.tmp");
        (|| -> std::io::Result<()> {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })()
        .map_err(|error| format!("could not save {}: {error}", path.display()))
    }

    /// Translate currently active physical bindings into a GBA key mask.
    pub fn mask(&self, active: &[Binding]) -> u32 {
        use mgba::input::keys;

        let bits = [
            keys::A,
            keys::B,
            keys::SELECT,
            keys::START,
            keys::RIGHT,
            keys::LEFT,
            keys::UP,
            keys::DOWN,
            keys::R,
            keys::L,
        ];
        self.bindings
            .iter()
            .zip(bits)
            .filter_map(|(binding, bit)| active.contains(binding).then_some(bit))
            .fold(0, |mask, bit| mask | bit)
    }
}

/// Load settings through a free function for callers that prefer functional
/// style. [`Settings::load`] is the canonical API.
pub fn load<P: AsRef<Path>>(path: P) -> (Settings, Option<String>) {
    Settings::load(path)
}

/// A decoded XInput state. Keeping this representation independent from the
/// Windows FFI makes input decoding testable and gives non-Windows callers a
/// useful no-op implementation of [`controller_inputs`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XInputState {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_stick_x: i16,
    pub left_stick_y: i16,
    pub right_stick_x: i16,
    pub right_stick_y: i16,
}

/// Stick deadzone in raw XInput units.
pub const STICK_DEADZONE: i16 = 16_000;
/// Trigger deadzone in raw XInput units.
pub const TRIGGER_DEADZONE: u8 = 128;

/// Decode one connected XInput state into active bindings. `None` denotes a
/// disconnected slot and intentionally decodes to an empty vector, releasing
/// any bindings that were active on the previous poll.
pub fn decode_xinput_state(controller: u8, state: Option<XInputState>) -> Vec<Binding> {
    let Some(state) = state else {
        return Vec::new();
    };
    let mut active = Vec::new();
    let buttons = [
        (XInputButton::DPadUp, XINPUT_GAMEPAD_DPAD_UP),
        (XInputButton::DPadDown, XINPUT_GAMEPAD_DPAD_DOWN),
        (XInputButton::DPadLeft, XINPUT_GAMEPAD_DPAD_LEFT),
        (XInputButton::DPadRight, XINPUT_GAMEPAD_DPAD_RIGHT),
        (XInputButton::Start, XINPUT_GAMEPAD_START),
        (XInputButton::Back, XINPUT_GAMEPAD_BACK),
        (XInputButton::LeftThumb, XINPUT_GAMEPAD_LEFT_THUMB),
        (XInputButton::RightThumb, XINPUT_GAMEPAD_RIGHT_THUMB),
        (XInputButton::LeftShoulder, XINPUT_GAMEPAD_LEFT_SHOULDER),
        (XInputButton::RightShoulder, XINPUT_GAMEPAD_RIGHT_SHOULDER),
        (XInputButton::A, XINPUT_GAMEPAD_A),
        (XInputButton::B, XINPUT_GAMEPAD_B),
        (XInputButton::X, XINPUT_GAMEPAD_X),
        (XInputButton::Y, XINPUT_GAMEPAD_Y),
    ];
    for (button, bit) in buttons {
        if state.buttons & bit != 0 {
            active.push(Binding::XInputButton { controller, button });
        }
    }

    let axes = [
        XInputAxis::LeftStickX,
        XInputAxis::LeftStickY,
        XInputAxis::RightStickX,
        XInputAxis::RightStickY,
    ];
    for axis in axes {
        let value = axis.value(state);
        let direction = if value > STICK_DEADZONE {
            Some(AxisDirection::Positive)
        } else if value < -STICK_DEADZONE {
            Some(AxisDirection::Negative)
        } else {
            None
        };
        if let Some(direction) = direction {
            active.push(Binding::XInputAxis {
                controller,
                axis,
                direction,
            });
        }
    }

    if state.left_trigger >= TRIGGER_DEADZONE {
        active.push(Binding::XInputTrigger {
            controller,
            trigger: XInputTrigger::Left,
        });
    }
    if state.right_trigger >= TRIGGER_DEADZONE {
        active.push(Binding::XInputTrigger {
            controller,
            trigger: XInputTrigger::Right,
        });
    }
    active
}

/// Return active controls for every connected XInput slot.
///
/// Windows uses the system `xinput1_4` library. Other platforms deliberately
/// return an empty vector; keyboard input remains available everywhere.
#[cfg(windows)]
pub fn controller_inputs() -> Vec<Binding> {
    use std::cell::RefCell;
    use std::time::{Duration, Instant};
    thread_local! {
        static RETRY: RefCell<[Option<Instant>; 4]> = const { RefCell::new([None; 4]) };
    }
    let mut active = Vec::new();
    RETRY.with(|retry| {
        let mut retry = retry.borrow_mut();
        let now = Instant::now();
        for controller in 0..4u8 {
            if retry[controller as usize].is_some_and(|deadline| now < deadline) {
                continue;
            }
            let mut raw = XInputStateRaw::default();
            // XInputGetState returns ERROR_SUCCESS (0) for a connected slot.
            let result = unsafe { XInputGetState(u32::from(controller), &mut raw) };
            if result == 0 {
                retry[controller as usize] = None;
                active.extend(decode_xinput_state(controller, Some(raw.into())));
            } else {
                retry[controller as usize] = Some(now + Duration::from_secs(1));
            }
        }
    });
    active
}

#[cfg(not(windows))]
pub fn controller_inputs() -> Vec<Binding> {
    Vec::new()
}

/// Poll the device driver away from the simulation thread, including hotplug
/// probes for disconnected slots. The shared lock covers only a snapshot swap.
pub struct Controllers {
    state: std::sync::Arc<std::sync::Mutex<Vec<Binding>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Controllers {
    pub fn new() -> Result<Self, String> {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        };
        let state = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let shared = state.clone();
        let stopping = stop.clone();
        let worker = std::thread::Builder::new()
            .name("netplay-controller".into())
            .spawn(move || {
                while !stopping.load(Ordering::Relaxed) {
                    let active = controller_inputs();
                    *shared.lock().unwrap() = active;
                    std::thread::sleep(std::time::Duration::from_millis(4));
                }
            })
            .map_err(|e| format!("Cannot start controller polling: {e}"))?;
        Ok(Self {
            state,
            stop,
            worker: Some(worker),
        })
    }

    pub fn snapshot(&self) -> Vec<Binding> {
        self.state.lock().unwrap().clone()
    }
}

impl Drop for Controllers {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct XInputGamepadRaw {
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    left_stick_x: i16,
    left_stick_y: i16,
    right_stick_x: i16,
    right_stick_y: i16,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct XInputStateRaw {
    packet_number: u32,
    gamepad: XInputGamepadRaw,
}

#[cfg(windows)]
impl From<XInputStateRaw> for XInputState {
    fn from(raw: XInputStateRaw) -> Self {
        Self {
            buttons: raw.gamepad.buttons,
            left_trigger: raw.gamepad.left_trigger,
            right_trigger: raw.gamepad.right_trigger,
            left_stick_x: raw.gamepad.left_stick_x,
            left_stick_y: raw.gamepad.left_stick_y,
            right_stick_x: raw.gamepad.right_stick_x,
            right_stick_y: raw.gamepad.right_stick_y,
        }
    }
}

#[cfg(windows)]
// The Windows SDK's xinput.lib imports the system XINPUT1_4.dll.
#[link(name = "xinput")]
extern "system" {
    fn XInputGetState(user_index: u32, state: *mut XInputStateRaw) -> u32;
}

/// The complete explicit minifb key table. `Count` is a sentinel rather than
/// a key and is intentionally not included.
pub const KEY_NAMES: &[(Key, &str)] = &[
    (Key::Key0, "Key0"),
    (Key::Key1, "Key1"),
    (Key::Key2, "Key2"),
    (Key::Key3, "Key3"),
    (Key::Key4, "Key4"),
    (Key::Key5, "Key5"),
    (Key::Key6, "Key6"),
    (Key::Key7, "Key7"),
    (Key::Key8, "Key8"),
    (Key::Key9, "Key9"),
    (Key::A, "A"),
    (Key::B, "B"),
    (Key::C, "C"),
    (Key::D, "D"),
    (Key::E, "E"),
    (Key::F, "F"),
    (Key::G, "G"),
    (Key::H, "H"),
    (Key::I, "I"),
    (Key::J, "J"),
    (Key::K, "K"),
    (Key::L, "L"),
    (Key::M, "M"),
    (Key::N, "N"),
    (Key::O, "O"),
    (Key::P, "P"),
    (Key::Q, "Q"),
    (Key::R, "R"),
    (Key::S, "S"),
    (Key::T, "T"),
    (Key::U, "U"),
    (Key::V, "V"),
    (Key::W, "W"),
    (Key::X, "X"),
    (Key::Y, "Y"),
    (Key::Z, "Z"),
    (Key::F1, "F1"),
    (Key::F2, "F2"),
    (Key::F3, "F3"),
    (Key::F4, "F4"),
    (Key::F5, "F5"),
    (Key::F6, "F6"),
    (Key::F7, "F7"),
    (Key::F8, "F8"),
    (Key::F9, "F9"),
    (Key::F10, "F10"),
    (Key::F11, "F11"),
    (Key::F12, "F12"),
    (Key::F13, "F13"),
    (Key::F14, "F14"),
    (Key::F15, "F15"),
    (Key::Down, "Down"),
    (Key::Left, "Left"),
    (Key::Right, "Right"),
    (Key::Up, "Up"),
    (Key::Apostrophe, "Apostrophe"),
    (Key::Backquote, "Backquote"),
    (Key::Backslash, "Backslash"),
    (Key::Comma, "Comma"),
    (Key::Equal, "Equal"),
    (Key::LeftBracket, "LeftBracket"),
    (Key::Minus, "Minus"),
    (Key::Period, "Period"),
    (Key::RightBracket, "RightBracket"),
    (Key::Semicolon, "Semicolon"),
    (Key::Slash, "Slash"),
    (Key::Backspace, "Backspace"),
    (Key::Delete, "Delete"),
    (Key::End, "End"),
    (Key::Enter, "Enter"),
    (Key::Escape, "Escape"),
    (Key::Home, "Home"),
    (Key::Insert, "Insert"),
    (Key::Menu, "Menu"),
    (Key::PageDown, "PageDown"),
    (Key::PageUp, "PageUp"),
    (Key::Pause, "Pause"),
    (Key::Space, "Space"),
    (Key::Tab, "Tab"),
    (Key::NumLock, "NumLock"),
    (Key::CapsLock, "CapsLock"),
    (Key::ScrollLock, "ScrollLock"),
    (Key::LeftShift, "LeftShift"),
    (Key::RightShift, "RightShift"),
    (Key::LeftCtrl, "LeftCtrl"),
    (Key::RightCtrl, "RightCtrl"),
    (Key::NumPad0, "NumPad0"),
    (Key::NumPad1, "NumPad1"),
    (Key::NumPad2, "NumPad2"),
    (Key::NumPad3, "NumPad3"),
    (Key::NumPad4, "NumPad4"),
    (Key::NumPad5, "NumPad5"),
    (Key::NumPad6, "NumPad6"),
    (Key::NumPad7, "NumPad7"),
    (Key::NumPad8, "NumPad8"),
    (Key::NumPad9, "NumPad9"),
    (Key::NumPadDot, "NumPadDot"),
    (Key::NumPadSlash, "NumPadSlash"),
    (Key::NumPadAsterisk, "NumPadAsterisk"),
    (Key::NumPadMinus, "NumPadMinus"),
    (Key::NumPadPlus, "NumPadPlus"),
    (Key::NumPadEnter, "NumPadEnter"),
    (Key::LeftAlt, "LeftAlt"),
    (Key::RightAlt, "RightAlt"),
    (Key::LeftSuper, "LeftSuper"),
    (Key::RightSuper, "RightSuper"),
    (Key::Unknown, "Unknown"),
];

/// Return minifb's stable, human-readable key name.
pub fn key_name(key: Key) -> &'static str {
    KEY_NAMES
        .iter()
        .find_map(|(candidate, name)| (*candidate == key).then_some(*name))
        .unwrap_or("Unknown")
}

/// Parse a minifb key name without relying on enum discriminants or unsafe
/// casts. Matching is case-insensitive.
pub fn parse_key_name(name: &str) -> Option<Key> {
    KEY_NAMES
        .iter()
        .find_map(|(key, candidate)| candidate.eq_ignore_ascii_case(name.trim()).then_some(*key))
}

/// Return all key/name pairs understood by the settings parser.
pub fn key_names() -> &'static [(Key, &'static str)] {
    KEY_NAMES
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_path(label: &str) -> std::path::PathBuf {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ss2-netplay-controls-{label}-{}-{}-{}.txt",
            std::process::id(),
            nanos,
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn defaults_match_documented_keyboard_layout() {
        let settings = Settings::default();
        assert_eq!(settings.volume, 100);
        assert_eq!(settings.bindings[0], Binding::Keyboard(Key::Z));
        assert_eq!(settings.bindings[2], Binding::Keyboard(Key::RightShift));
        assert_eq!(settings.bindings[9], Binding::Keyboard(Key::C));
    }

    #[test]
    fn settings_round_trip_preserves_every_binding_kind() {
        let path = temporary_path("roundtrip");
        let settings = Settings {
            volume: 37,
            bindings: [
                Binding::Keyboard(Key::A),
                Binding::Keyboard(Key::F15),
                Binding::XInputButton {
                    controller: 0,
                    button: XInputButton::DPadUp,
                },
                Binding::XInputButton {
                    controller: 3,
                    button: XInputButton::RightShoulder,
                },
                Binding::XInputAxis {
                    controller: 1,
                    axis: XInputAxis::LeftStickX,
                    direction: AxisDirection::Negative,
                },
                Binding::XInputAxis {
                    controller: 2,
                    axis: XInputAxis::RightStickY,
                    direction: AxisDirection::Positive,
                },
                Binding::XInputTrigger {
                    controller: 0,
                    trigger: XInputTrigger::Left,
                },
                Binding::XInputTrigger {
                    controller: 3,
                    trigger: XInputTrigger::Right,
                },
                Binding::Keyboard(Key::NumPadAsterisk),
                Binding::Keyboard(Key::Unknown),
            ],
        };
        settings.save(&path).expect("save settings");
        let (loaded, warning) = Settings::load(&path);
        assert_eq!(warning, None);
        assert_eq!(loaded, settings);
        fs::remove_file(path).expect("remove settings");
    }

    #[test]
    fn malformed_settings_are_reported_without_overwriting_the_file() {
        let path = temporary_path("malformed");
        let original = "SS2_SETTINGS 1\nvolume=too-loud\nbinding.A=keyboard:NoSuchKey\n";
        fs::write(&path, original).expect("write malformed settings");
        let (loaded, warning) = Settings::load(&path);
        assert!(warning.is_some());
        assert_eq!(loaded.volume, 100);
        assert_eq!(loaded.bindings[0], Binding::Keyboard(Key::Z));
        assert_eq!(fs::read_to_string(&path).expect("read settings"), original);
        fs::remove_file(path).expect("remove settings");
    }

    #[test]
    fn every_minifb_key_has_a_safe_round_trip_name() {
        assert_eq!(KEY_NAMES.len(), 107);
        for (key, name) in KEY_NAMES {
            assert_eq!(parse_key_name(name), Some(*key), "key {name}");
            assert_eq!(key_name(*key), *name);
            assert_eq!(
                Binding::parse_config_name(&format!("keyboard:{name}")),
                Some(Binding::Keyboard(*key))
            );
        }
    }

    #[test]
    fn xinput_axes_use_deadzone_and_disconnect_releases_inputs() {
        let neutral = XInputState {
            left_stick_x: STICK_DEADZONE,
            left_stick_y: -STICK_DEADZONE,
            right_trigger: TRIGGER_DEADZONE - 1,
            ..XInputState::default()
        };
        assert!(decode_xinput_state(0, Some(neutral)).is_empty());

        let active = XInputState {
            left_stick_x: STICK_DEADZONE + 1,
            left_stick_y: -STICK_DEADZONE - 1,
            right_trigger: TRIGGER_DEADZONE,
            ..XInputState::default()
        };
        let bindings = decode_xinput_state(2, Some(active));
        assert!(bindings.contains(&Binding::XInputAxis {
            controller: 2,
            axis: XInputAxis::LeftStickX,
            direction: AxisDirection::Positive,
        }));
        assert!(bindings.contains(&Binding::XInputAxis {
            controller: 2,
            axis: XInputAxis::LeftStickY,
            direction: AxisDirection::Negative,
        }));
        assert!(bindings.contains(&Binding::XInputTrigger {
            controller: 2,
            trigger: XInputTrigger::Right,
        }));
        assert!(decode_xinput_state(2, None).is_empty());
    }

    #[test]
    fn mapped_controller_inputs_use_the_bound_slot_and_release_on_disconnect() {
        let mut settings = Settings::default();
        settings.bindings[3] = Binding::XInputButton {
            controller: 1,
            button: XInputButton::Start,
        };
        settings.bindings[4] = Binding::XInputAxis {
            controller: 1,
            axis: XInputAxis::LeftStickX,
            direction: AxisDirection::Positive,
        };
        settings.bindings[9] = Binding::XInputTrigger {
            controller: 1,
            trigger: XInputTrigger::Left,
        };
        let state = XInputState {
            buttons: XINPUT_GAMEPAD_START,
            left_stick_x: i16::MAX,
            left_trigger: 255,
            ..XInputState::default()
        };
        assert_eq!(
            settings.mask(&decode_xinput_state(1, Some(state))),
            mgba::input::keys::START | mgba::input::keys::RIGHT | mgba::input::keys::L
        );
        assert_eq!(settings.mask(&decode_xinput_state(0, Some(state))), 0);
        assert_eq!(settings.mask(&decode_xinput_state(1, None)), 0);
    }

    #[test]
    fn settings_mask_matches_action_order_and_shared_bindings() {
        let settings = Settings::default();
        let active = vec![
            Binding::Keyboard(Key::Z),
            Binding::Keyboard(Key::Right),
            Binding::Keyboard(Key::C),
        ];
        let mask = settings.mask(&active);
        use mgba::input::keys;
        assert_eq!(mask, keys::A | keys::RIGHT | keys::L);
    }
}
