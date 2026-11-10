//! A declarative shape for unvalidated text newtypes.
//!
//! A text newtype here carries meaning, not a rule: anything is accepted, and
//! the type only stops one kind of text being passed where another is meant.
//! A value with a rule gets a hand-written parser in its own module instead.

/// Declares a text newtype over `String` with the derived std surface:
/// `Display`, `From<String>`, `From<&str>`, `AsRef<str>`, `as_str`, and transparent serde
/// both ways.
macro_rules! text_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            derive_more::Display,
            derive_more::From,
            derive_more::AsRef,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        #[as_ref(str)]
        pub struct $name(String);

        impl From<&str> for $name {
            fn from(text: &str) -> Self {
                Self(text.to_owned())
            }
        }

        impl $name {
            /// The text.
            #[allow(dead_code)]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

pub(crate) use text_newtype;
