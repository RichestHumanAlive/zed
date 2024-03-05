use anyhow::anyhow;
use serde::Deserialize;
use smallvec::SmallVec;
use std::fmt::Write;

/// A keystroke and associated metadata generated by the platform
#[derive(Clone, Debug, Eq, PartialEq, Default, Deserialize, Hash)]
pub struct Keystroke {
    /// the state of the modifier keys at the time the keystroke was generated
    pub modifiers: Modifiers,

    /// key is the character printed on the key that was pressed
    /// e.g. for option-s, key is "s"
    pub key: String,

    /// ime_key is the character inserted by the IME engine when that key was pressed.
    /// e.g. for option-s, ime_key is "ß"
    pub ime_key: Option<String>,
}

impl Keystroke {
    /// When matching a key we cannot know whether the user intended to type
    /// the ime_key or the key itself. On some non-US keyboards keys we use in our
    /// bindings are behind option (for example `$` is typed `alt-ç` on a Czech keyboard),
    /// and on some keyboards the IME handler converts a sequence of keys into a
    /// specific character (for example `"` is typed as `" space` on a brazilian keyboard).
    ///
    /// This method generates a list of potential keystroke candidates that could be matched
    /// against when resolving a keybinding.
    pub(crate) fn match_candidates(&self) -> SmallVec<[Keystroke; 2]> {
        let mut possibilities = SmallVec::new();
        match self.ime_key.as_ref() {
            Some(ime_key) => {
                if ime_key != &self.key {
                    possibilities.push(Keystroke {
                        modifiers: Modifiers {
                            control: self.modifiers.control,
                            alt: false,
                            shift: false,
                            command: false,
                            function: false,
                        },
                        key: ime_key.to_string(),
                        ime_key: None,
                    });
                }
                possibilities.push(Keystroke {
                    ime_key: None,
                    ..self.clone()
                });
            }
            None => possibilities.push(self.clone()),
        }
        possibilities
    }

    /// key syntax is:
    /// [ctrl-][alt-][shift-][cmd-][fn-]key[->ime_key]
    /// ime_key syntax is only used for generating test events,
    /// when matching a key with an ime_key set will be matched without it.
    pub fn parse(source: &str) -> anyhow::Result<Self> {
        let mut control = false;
        let mut alt = false;
        let mut shift = false;
        let mut command = false;
        let mut function = false;
        let mut key = None;
        let mut ime_key = None;

        let mut components = source.split('-').peekable();
        while let Some(component) = components.next() {
            match component {
                "ctrl" => control = true,
                "alt" => alt = true,
                "shift" => shift = true,
                "cmd" => command = true,
                "fn" => function = true,
                _ => {
                    if let Some(next) = components.peek() {
                        if next.is_empty() && source.ends_with('-') {
                            key = Some(String::from("-"));
                            break;
                        } else if next.len() > 1 && next.starts_with('>') {
                            key = Some(String::from(component));
                            ime_key = Some(String::from(&next[1..]));
                            components.next();
                        } else {
                            return Err(anyhow!("Invalid keystroke `{}`", source));
                        }
                    } else {
                        key = Some(String::from(component));
                    }
                }
            }
        }

        let key = key.ok_or_else(|| anyhow!("Invalid keystroke `{}`", source))?;

        Ok(Keystroke {
            modifiers: Modifiers {
                control,
                alt,
                shift,
                command,
                function,
            },
            key,
            ime_key,
        })
    }

    /// Returns a new keystroke with the ime_key filled.
    /// This is used for dispatch_keystroke where we want users to
    /// be able to simulate typing "space", etc.
    pub fn with_simulated_ime(mut self) -> Self {
        if self.ime_key.is_none()
            && !self.modifiers.command
            && !self.modifiers.control
            && !self.modifiers.function
            && !self.modifiers.alt
        {
            self.ime_key = match self.key.as_str() {
                "space" => Some(" ".into()),
                "tab" => Some("\t".into()),
                "enter" => Some("\n".into()),
                "up" | "down" | "left" | "right" | "pageup" | "pagedown" | "home" | "end"
                | "delete" | "escape" | "backspace" | "f1" | "f2" | "f3" | "f4" | "f5" | "f6"
                | "f7" | "f8" | "f9" | "f10" | "f11" | "f12" => None,
                key => {
                    if self.modifiers.shift {
                        Some(key.to_uppercase())
                    } else {
                        Some(key.into())
                    }
                }
            }
        }
        self
    }
}

impl std::fmt::Display for Keystroke {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.modifiers.control {
            f.write_char('^')?;
        }
        if self.modifiers.alt {
            f.write_char('⌥')?;
        }
        if self.modifiers.command {
            f.write_char('⌘')?;
        }
        if self.modifiers.shift {
            f.write_char('⇧')?;
        }
        let key = match self.key.as_str() {
            "backspace" => '⌫',
            "up" => '↑',
            "down" => '↓',
            "left" => '←',
            "right" => '→',
            "tab" => '⇥',
            "escape" => '⎋',
            key => {
                if key.len() == 1 {
                    key.chars().next().unwrap().to_ascii_uppercase()
                } else {
                    return f.write_str(key);
                }
            }
        };
        f.write_char(key)
    }
}

/// The state of the modifier keys at some point in time
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default, Deserialize, Hash)]
pub struct Modifiers {
    /// The control key
    pub control: bool,

    /// The alt key
    /// Sometimes also known as the 'meta' key
    pub alt: bool,

    /// The shift key
    pub shift: bool,

    /// The command key, on macos
    /// the windows key, on windows
    pub command: bool,

    /// The function key
    pub function: bool,
}

impl Modifiers {
    /// Returns true if any modifier key is pressed
    pub fn modified(&self) -> bool {
        self.control || self.alt || self.shift || self.command || self.function
    }

    /// helper method for Modifiers with no modifiers
    pub fn none() -> Modifiers {
        Default::default()
    }

    /// helper method for Modifiers with just command
    pub fn command() -> Modifiers {
        Modifiers {
            command: true,
            ..Default::default()
        }
    }

    /// helper method for Modifiers with just shift
    pub fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Default::default()
        }
    }

    /// helper method for Modifiers with command + shift
    pub fn command_shift() -> Modifiers {
        Modifiers {
            shift: true,
            command: true,
            ..Default::default()
        }
    }
}
