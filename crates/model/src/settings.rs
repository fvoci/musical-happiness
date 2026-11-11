//! What a user has set about how things are shown to them, and the small bag of state a
//! client keeps with the account.
//!
//! The settings are a closed set: the server knows every one, its values and its
//! default, and refuses anything else; of a language it knows only the shape of a tag. They belong to the account and not to a device,
//! so that the same person sees the same times on every device.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use serde::Serialize;

use crate::JsonMap;

/// The text is no value of the setting it was read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a value of this setting")]
pub struct ParseSettingError;

/// Defines a setting whose values are a closed set of words.
macro_rules! word_setting {
    (
        $(#[doc = $doc:literal])*
        $name:ident { $($(#[$variant_meta:meta])* $variant:ident => $word:literal),+ $(,)? }
    ) => {
        $(#[doc = $doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
        pub enum $name {
            $(
                $(#[$variant_meta])*
                #[serde(rename = $word)]
                $variant,
            )+
        }

        impl $name {
            /// Every value, as it is written.
            pub const WORDS: &'static [&'static str] = &[$($word),+];

            /// The value as it is written.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $word,)+
                }
            }
        }

        // Written out and not derived: the derive reads the name of a variant where it
        // cannot read the `rename` this macro writes, and would list `Auto` where the
        // value is `auto`.
        impl utoipa::PartialSchema for $name {
            fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                let description = [$($doc),*].map(str::trim).join("\n");
                utoipa::openapi::ObjectBuilder::new()
                    .schema_type(utoipa::openapi::schema::Type::String)
                    .enum_values(Some(Self::WORDS.iter().copied()))
                    .description(Some(description))
                    .into()
            }
        }

        impl utoipa::ToSchema for $name {
            fn name() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed(stringify!($name))
            }
        }

        impl FromStr for $name {
            type Err = ParseSettingError;

            fn from_str(word: &str) -> Result<Self, ParseSettingError> {
                match word {
                    $($word => Ok(Self::$variant),)+
                    _ => Err(ParseSettingError),
                }
            }
        }
    };
}

word_setting! {
    /// The day a week starts on.
    WeekStart {
        /// As the device's locale has it.
        #[default]
        Auto => "auto",
        /// Sunday.
        Sunday => "sunday",
        /// Monday.
        Monday => "monday",
        /// Saturday.
        Saturday => "saturday",
    }
}

word_setting! {
    /// How the hours of a day are written. The names are those of the browsers' `Intl`.
    HourCycle {
        /// As the device's locale has it.
        #[default]
        Auto => "auto",
        /// 1 to 12, with a.m. and p.m.
        H12 => "h12",
        /// 0 to 23.
        H23 => "h23",
    }
}

word_setting! {
    /// The colors of the interface.
    Theme {
        /// As the device has it.
        #[default]
        Auto => "auto",
        /// Light.
        Light => "light",
        /// Dark.
        Dark => "dark",
    }
}

word_setting! {
    /// Where a card opens.
    CardOpen {
        /// In a dialog in the middle of the board.
        #[default]
        Center => "center",
        /// In a panel at the side of the board.
        Side => "side",
        /// As a page of its own.
        Full => "full",
    }
}

word_setting! {
    /// How much room the interface gives each thing.
    Density {
        /// As fits the device.
        #[default]
        Auto => "auto",
        /// Less room: more on the screen.
        Compact => "compact",
        /// More room.
        Comfortable => "comfortable",
    }
}

/// The time zone times are shown in: that of the device, or one by its IANA name.
///
/// A name is one the time zone database compiled into this build knows, spelled as the
/// database spells it (names are case-sensitive). Names the database keeps for
/// compatibility (`Asia/Calcutta`) are names too, and are kept as given.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct TimeZone(Option<String>);

impl TimeZone {
    /// The word for "the time zone of the device".
    pub const AUTO: &'static str = "auto";

    /// The setting as it is written: `auto` or the name of the time zone.
    pub fn as_str(&self) -> &str {
        self.0.as_deref().unwrap_or(Self::AUTO)
    }
}

impl FromStr for TimeZone {
    type Err = ParseSettingError;

    fn from_str(text: &str) -> Result<Self, ParseSettingError> {
        if text == Self::AUTO {
            Ok(Self(None))
        } else if text.parse::<chrono_tz::Tz>().is_ok() {
            Ok(Self(Some(text.to_owned())))
        } else {
            Err(ParseSettingError)
        }
    }
}

/// The language of the interface: that of the browser, or one by its language tag.
///
/// The server keeps no list of languages; it checks that a tag has the shape the
/// browsers' `Intl` reads (a Unicode BCP 47 locale identifier, `-` between subtags):
/// a language of 2–3 or 5–8 letters, then an optional script of 4 letters, an optional
/// region of 2 letters or 3 digits, variants of 5–8 letters and digits or a digit and 3
/// (none twice), extensions (a letter or digit other than `x`, then subtags of 2–8) and
/// private use (`x`, then subtags of 1–8) last. At most [`Language::MAX_LENGTH`]
/// characters. A tag is kept as given, case and all (`en-us` stays `en-us`; tags do not
/// differ by case).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Language(Option<String>);

impl Language {
    /// The word for "the language of the browser".
    pub const AUTO: &'static str = "auto";
    /// The longest tag the server takes: room for a tag with a `-u-` extension, such as
    /// what `Intl.Locale` writes.
    pub const MAX_LENGTH: usize = 64;

    /// The setting as it is written: `auto` or the tag.
    pub fn as_str(&self) -> &str {
        self.0.as_deref().unwrap_or(Self::AUTO)
    }
}

/// Whether `tag` has the shape of a Unicode BCP 47 locale identifier.
fn is_locale_tag(tag: &str) -> bool {
    let letters = |s: &str, lengths: std::ops::RangeInclusive<usize>| {
        lengths.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphabetic())
    };
    let alphanumerics = |s: &str, lengths: std::ops::RangeInclusive<usize>| {
        lengths.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric())
    };
    let mut subtags = tag.split('-').peekable();
    let language = subtags.next().unwrap_or_default();
    if !(letters(language, 2..=3) || letters(language, 5..=8)) {
        return false;
    }
    let _script = subtags.next_if(|s| letters(s, 4..=4));
    let is_region =
        |s: &&str| letters(s, 2..=2) || (s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()));
    let _region = subtags.next_if(is_region);
    let is_variant = |s: &&str| {
        alphanumerics(s, 5..=8)
            || (s.len() == 4 && s.as_bytes()[0].is_ascii_digit() && alphanumerics(s, 4..=4))
    };
    let mut variants = BTreeSet::new();
    while let Some(variant) = subtags.next_if(is_variant) {
        if !variants.insert(variant.to_ascii_lowercase()) {
            return false;
        }
    }
    let mut singletons = BTreeSet::new();
    while let Some(singleton) = subtags.next() {
        if !alphanumerics(singleton, 1..=1) || !singletons.insert(singleton.to_ascii_lowercase()) {
            return false;
        }
        let private = singleton.eq_ignore_ascii_case("x");
        let lengths = if private { 1..=8 } else { 2..=8 };
        let mut any = false;
        while subtags
            .next_if(|s| alphanumerics(s, lengths.clone()))
            .is_some()
        {
            any = true;
        }
        if !any {
            return false;
        }
        // Private use is last: whatever follows it is part of it.
        if private {
            return subtags.next().is_none();
        }
    }
    true
}

impl FromStr for Language {
    type Err = ParseSettingError;

    fn from_str(text: &str) -> Result<Self, ParseSettingError> {
        if text == Self::AUTO {
            Ok(Self(None))
        } else if text.len() <= Self::MAX_LENGTH && is_locale_tag(text) {
            Ok(Self(Some(text.to_owned())))
        } else {
            Err(ParseSettingError)
        }
    }
}

impl fmt::Debug for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Language({})", self.as_str())
    }
}

impl Serialize for Language {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl fmt::Debug for TimeZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TimeZone({})", self.as_str())
    }
}

impl Serialize for TimeZone {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The settings of a user. Every member has a value: what was never set, or was reset,
/// is at its default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserSettings {
    /// The time zone times are shown in.
    #[schema(value_type = String)]
    pub time_zone: TimeZone,
    /// The day a week starts on.
    pub week_start: WeekStart,
    /// How the hours of a day are written.
    pub hour_cycle: HourCycle,
    /// The colors of the interface.
    pub theme: Theme,
    /// Where a card opens.
    pub card_open: CardOpen,
    /// How much room the interface gives each thing.
    pub density: Density,
    /// Whether a device that can do so answers touches with a vibration.
    pub haptics: bool,
    /// The language of the interface: `auto` for the browser's, or a language tag such
    /// as `ko` or `en-US`, in the shape the browsers' `Intl` reads (Unicode BCP 47), at
    /// most 64 characters. The server keeps no list of languages, and answers a tag as
    /// it was given, case and all: compare tags without regard to case.
    #[schema(value_type = String)]
    pub language: Language,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            time_zone: TimeZone::default(),
            week_start: WeekStart::default(),
            hour_cycle: HourCycle::default(),
            theme: Theme::default(),
            card_open: CardOpen::default(),
            density: Density::default(),
            haptics: true,
            language: Language::default(),
        }
    }
}

/// One of the settings, by its name on the wire and in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SettingName {
    /// `timeZone`
    TimeZone,
    /// `weekStart`
    WeekStart,
    /// `hourCycle`
    HourCycle,
    /// `theme`
    Theme,
    /// `cardOpen`
    CardOpen,
    /// `density`
    Density,
    /// `haptics`
    Haptics,
    /// `language`
    Language,
}

impl SettingName {
    /// Every setting there is.
    pub const ALL: [Self; 8] = [
        Self::TimeZone,
        Self::WeekStart,
        Self::HourCycle,
        Self::Theme,
        Self::CardOpen,
        Self::Density,
        Self::Haptics,
        Self::Language,
    ];

    /// The name as it is written.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TimeZone => "timeZone",
            Self::WeekStart => "weekStart",
            Self::HourCycle => "hourCycle",
            Self::Theme => "theme",
            Self::CardOpen => "cardOpen",
            Self::Density => "density",
            Self::Haptics => "haptics",
            Self::Language => "language",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|known| known.as_str() == name)
    }

    /// What a value of this setting may be, for the message of a refusal.
    fn expectation(self) -> String {
        let words = |words: &[&str]| {
            let quoted: Vec<String> = words.iter().map(|word| format!("\"{word}\"")).collect();
            format!("one of {}", quoted.join(", "))
        };
        match self {
            Self::TimeZone => "\"auto\" or the name of an IANA time zone".to_owned(),
            Self::WeekStart => words(WeekStart::WORDS),
            Self::HourCycle => words(HourCycle::WORDS),
            Self::Theme => words(Theme::WORDS),
            Self::CardOpen => words(CardOpen::WORDS),
            Self::Density => words(Density::WORDS),
            Self::Haptics => "true or false".to_owned(),
            Self::Language => "\"auto\" or a language tag such as \"ko\" or \"en-US\"".to_owned(),
        }
    }
}

impl UserSettings {
    /// The settings as stored values say them: `(name, value)` pairs as
    /// [`UserSettingsPatch::changes`] produced them. A name this build does not know and
    /// a value it would not accept are passed over, and the setting is at its default: a
    /// value written by another build must not take the settings down.
    pub fn from_stored<'a>(stored: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut settings = Self::default();
        for (name, value) in stored {
            if let Some(name) = SettingName::from_name(name) {
                // A stored value that does not parse leaves the default in place.
                let _unreadable: Result<(), ParseSettingError> = settings.set(name, value);
            }
        }
        settings
    }

    /// Sets one setting from its stored form.
    fn set(&mut self, name: SettingName, value: &str) -> Result<(), ParseSettingError> {
        match name {
            SettingName::TimeZone => self.time_zone = value.parse()?,
            SettingName::WeekStart => self.week_start = value.parse()?,
            SettingName::HourCycle => self.hour_cycle = value.parse()?,
            SettingName::Theme => self.theme = value.parse()?,
            SettingName::CardOpen => self.card_open = value.parse()?,
            SettingName::Density => self.density = value.parse()?,
            SettingName::Haptics => {
                self.haptics = value.parse().map_err(|_| ParseSettingError)?;
            }
            SettingName::Language => self.language = value.parse()?,
        }
        Ok(())
    }
}

/// A change to a user's settings: some set to a value, some put back to their default.
/// The settings it does not name stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserSettingsPatch {
    set: BTreeMap<SettingName, String>,
    reset: BTreeSet<SettingName>,
}

/// Why a change to the settings was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    /// The request names a setting there is not.
    #[error("there is no setting {name:?}")]
    UnknownSetting {
        /// The name the request used.
        name: String,
    },
    /// The value is none the setting can have.
    #[error("{name} must be {expected}")]
    InvalidValue {
        /// The setting, by its name on the wire.
        name: &'static str,
        /// What a value of the setting may be.
        expected: String,
    },
}

impl SettingsError {
    /// The member of the request that is wrong.
    pub fn field(&self) -> &str {
        match self {
            Self::UnknownSetting { name } => name,
            Self::InvalidValue { name, .. } => name,
        }
    }
}

impl UserSettingsPatch {
    /// Reads a change from the members of a request: a value sets the setting, `null`
    /// puts it back to its default. Every member must be a setting, and every value one
    /// the setting can have.
    ///
    /// # Errors
    ///
    /// Returns the first member, in the order of the names, that is no setting or whose
    /// value the setting cannot have.
    pub fn from_members(members: &JsonMap) -> Result<Self, SettingsError> {
        let mut patch = Self::default();
        for (name, value) in members {
            let setting = SettingName::from_name(name)
                .ok_or_else(|| SettingsError::UnknownSetting { name: name.clone() })?;
            let invalid = || SettingsError::InvalidValue {
                name: setting.as_str(),
                expected: setting.expectation(),
            };
            let stored = match (setting, value) {
                (_, serde_json::Value::Null) => {
                    patch.reset.insert(setting);
                    continue;
                }
                (SettingName::Haptics, serde_json::Value::Bool(flag)) => flag.to_string(),
                (SettingName::Haptics, _) => return Err(invalid()),
                (_, serde_json::Value::String(text)) => text.clone(),
                _ => return Err(invalid()),
            };
            // Whatever is stored must read back: checked with the reader itself.
            UserSettings::default()
                .set(setting, &stored)
                .map_err(|ParseSettingError| invalid())?;
            patch.set.insert(setting, stored);
        }
        Ok(patch)
    }

    /// Whether the change changes nothing.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.reset.is_empty()
    }

    /// The settings to store, as `(name, value)`, and the names of the settings whose
    /// stored value is to be removed, each in the order the settings are listed in.
    pub fn changes(&self) -> (Vec<(&'static str, &str)>, Vec<&'static str>) {
        let set = self
            .set
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let reset = self.reset.iter().map(|name| name.as_str()).collect();
        (set, reset)
    }
}

/// The most entries the bag of client state of one user holds.
pub const CLIENT_STATE_MAX_ENTRIES: usize = 100;
/// The longest name of an entry of the bag of client state, in characters.
pub const CLIENT_STATE_MAX_NAME_LENGTH: usize = 64;
/// The longest value of an entry of the bag of client state, in bytes.
pub const CLIENT_STATE_MAX_VALUE_BYTES: usize = 4 * 1024;

/// Why an entry of the bag of client state was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientStateError {
    /// The name is empty, too long, or has a character that is not a letter of the
    /// English alphabet, a digit, `_`, `.` or `-`.
    #[error(
        "the name of an entry is 1 to {CLIENT_STATE_MAX_NAME_LENGTH} of the characters \
         A-Z, a-z, 0-9, '_', '.' and '-'"
    )]
    InvalidName,
    /// The value is too long.
    #[error("the value of an entry is at most {CLIENT_STATE_MAX_VALUE_BYTES} bytes")]
    ValueTooLong,
}

/// Checks the name of an entry of the bag of client state.
///
/// # Errors
///
/// Returns [`ClientStateError::InvalidName`] if the name is none.
pub fn check_client_state_name(name: &str) -> Result<(), ClientStateError> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-');
    if name.is_empty() || name.len() > CLIENT_STATE_MAX_NAME_LENGTH || !name.chars().all(allowed) {
        Err(ClientStateError::InvalidName)
    } else {
        Ok(())
    }
}

/// Checks the value of an entry of the bag of client state.
///
/// # Errors
///
/// Returns [`ClientStateError::ValueTooLong`] if the value is too long.
pub fn check_client_state_value(value: &str) -> Result<(), ClientStateError> {
    if value.len() > CLIENT_STATE_MAX_VALUE_BYTES {
        Err(ClientStateError::ValueTooLong)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn members(value: serde_json::Value) -> JsonMap {
        match value {
            serde_json::Value::Object(members) => members,
            other => panic!("the test's body must be an object, not {other}"),
        }
    }

    fn patched(value: serde_json::Value) -> UserSettings {
        let patch = UserSettingsPatch::from_members(&members(value)).expect("a valid change");
        let (set, _reset) = patch.changes();
        UserSettings::from_stored(set)
    }

    #[test]
    fn the_defaults_are_what_is_answered_when_nothing_is_stored() {
        assert_eq!(
            serde_json::to_value(UserSettings::from_stored([])).expect("settings are JSON"),
            json!({
                "timeZone": "auto", "weekStart": "auto", "hourCycle": "auto",
                "theme": "auto", "cardOpen": "center", "density": "auto", "haptics": true,
                "language": "auto",
            })
        );
    }

    #[test]
    fn every_value_of_every_setting_is_stored_and_read_back() {
        let cases = [
            ("weekStart", WeekStart::WORDS),
            ("hourCycle", HourCycle::WORDS),
            ("theme", Theme::WORDS),
            ("cardOpen", CardOpen::WORDS),
            ("density", Density::WORDS),
            (
                "timeZone",
                &[
                    "auto",
                    "Asia/Seoul",
                    "UTC",
                    "America/Argentina/Buenos_Aires",
                ][..],
            ),
        ];
        for (name, words) in cases {
            for word in words {
                let settings = patched(json!({name: word}));
                let answered = serde_json::to_value(&settings).expect("settings are JSON");
                assert_eq!(answered[name], json!(word), "{name} = {word}");
            }
        }
        for flag in [true, false] {
            assert_eq!(patched(json!({"haptics": flag})).haptics, flag, "haptics");
        }
        for tag in ["auto", "ko", "en-US", "zh-Hant-TW"] {
            assert_eq!(
                patched(json!({"language": tag})).language.as_str(),
                tag,
                "language"
            );
        }
    }

    #[test]
    fn a_name_the_database_keeps_for_compatibility_is_kept_as_given() {
        let settings = patched(json!({"timeZone": "Asia/Calcutta"}));
        assert_eq!(settings.time_zone.as_str(), "Asia/Calcutta");
    }

    #[test]
    fn what_is_no_value_of_a_setting_is_refused_with_its_name() {
        let cases = [
            json!({"timeZone": "Mars/Olympus_Mons"}),
            json!({"timeZone": "asia/seoul"}),
            json!({"timeZone": "Auto"}),
            json!({"timeZone": ""}),
            json!({"timeZone": 9}),
            json!({"weekStart": "tuesday"}),
            json!({"weekStart": "Monday"}),
            json!({"hourCycle": "h24"}),
            json!({"hourCycle": 12}),
            json!({"theme": "sepia"}),
            json!({"theme": true}),
            json!({"cardOpen": "auto"}),
            json!({"cardOpen": ["side"]}),
            json!({"density": "cozy"}),
            json!({"haptics": "true"}),
            json!({"haptics": 1}),
            json!({"haptics": "auto"}),
        ];
        for case in cases {
            let name = case.as_object().expect("an object").keys().next().cloned();
            let error = UserSettingsPatch::from_members(&members(case.clone()))
                .expect_err("no value of the setting");
            assert!(
                matches!(error, SettingsError::InvalidValue { .. }),
                "{case}: {error:?}"
            );
            assert_eq!(Some(error.field()), name.as_deref(), "{case}");
        }
        let message = UserSettingsPatch::from_members(&members(json!({"cardOpen": "auto"})))
            .expect_err("no value")
            .to_string();
        assert_eq!(
            message, "cardOpen must be one of \"center\", \"side\", \"full\"",
            "the refusal says what the setting can be"
        );
    }

    #[test]
    fn a_name_that_is_no_setting_is_refused() {
        for name in ["timezone", "locale", "", "TimeZone"] {
            let error = UserSettingsPatch::from_members(&members(json!({name: "auto"})))
                .expect_err("no such setting");
            assert_eq!(
                error,
                SettingsError::UnknownSetting {
                    name: name.to_owned()
                }
            );
            assert_eq!(error.field(), name);
        }
    }

    #[test]
    fn null_puts_a_setting_back_and_what_is_not_named_is_not_touched() {
        let patch = UserSettingsPatch::from_members(&members(
            json!({"theme": null, "density": "compact", "haptics": null}),
        ))
        .expect("a valid change");
        let (set, reset) = patch.changes();
        assert_eq!(set, [("density", "compact")]);
        assert_eq!(reset, ["theme", "haptics"]);
        assert!(!patch.is_empty(), "it changes something");
        assert!(
            UserSettingsPatch::from_members(&JsonMap::new())
                .expect("nothing is a valid change")
                .is_empty(),
            "an empty object changes nothing"
        );
    }

    #[test]
    fn stored_values_this_build_cannot_read_leave_the_defaults() {
        let settings = UserSettings::from_stored([
            ("theme", "sepia"),
            ("locale", "ko"),
            ("haptics", "maybe"),
            ("timeZone", "Mars/Olympus_Mons"),
            ("density", "compact"),
        ]);
        assert_eq!(
            settings,
            UserSettings {
                density: Density::Compact,
                ..UserSettings::default()
            }
        );
    }

    #[test]
    fn names_and_values_of_client_state_are_bounded() {
        for name in ["a", "welcomePageViewed", "tour.step-1_x", &"n".repeat(64)] {
            assert_eq!(check_client_state_name(name), Ok(()), "{name}");
        }
        for name in ["", &"n".repeat(65), "with space", "키", "a/b", "a\n"] {
            assert_eq!(
                check_client_state_name(name),
                Err(ClientStateError::InvalidName),
                "{name:?}"
            );
        }
        assert_eq!(check_client_state_value(&"v".repeat(4096)), Ok(()));
        assert_eq!(
            check_client_state_value(&"v".repeat(4097)),
            Err(ClientStateError::ValueTooLong)
        );
    }

    #[test]
    fn a_language_is_auto_or_has_the_shape_of_a_tag() {
        for tag in [
            "auto",
            "ko",
            "en-US",
            "en-us",
            "zh-Hant-TW",
            "es-419",
            "sr-Latn",
            "de-CH-1901",
            "sl-rozaj-biske",
            "en-US-u-ca-gregory-hc-h23",
            "en-x-private",
            "haw",
        ] {
            let read: Language = tag.parse().expect(tag);
            assert_eq!(read.as_str(), tag, "kept as given");
        }
        for tag in [
            "",
            "k",
            "-",
            "ko-",
            "-ko",
            "ko--KR",
            "ko_KR",
            "한국어",
            "ko KR",
            "toolongtag",
            "en-toolongsub",
            "1a",
            &format!("en-u{}", "-abcdefgh".repeat(7)),
            "Auto",
            "AUTO",
            "abcd",
            "en-a",
            "en-x",
            "en-1",
            "en-US-US",
            "de-1901-1901",
            "en-a-bc-a-de",
            "en-x-ninechars",
            "zh-yue-HK",
            "sgn-BE-FR",
            "i-klingon",
            "x-private",
        ] {
            assert!(tag.parse::<Language>().is_err(), "{tag:?} is no tag");
        }
    }
}
