use pastey::paste;

#[macro_export]
macro_rules! define_it {
    (
        $(#[$attr_meta:meta])*
        $v:vis enum $name:ident {
            $(#[$other_attr_meta:meta])*
            $other:ident($inner:ty),
            $(
                $(#[$ident_attr_meta:meta])*
                $idents:ident
            ),* $(,)?
        }
    ) => {
        $(#[$attr_meta])*
        $v enum $name {
            $(#[$other_attr_meta])*
            $other($inner),
            $(
                $(#[$ident_attr_meta])*
                $idents,
            )*
        }

        // -------------------------
        // const items
        // -------------------------
        impl $name {
            pub const ITEMS: &'static [Self] = &[
                $(Self::$idents,)*
            ];
            pub const ITEMS_COUNT: usize = Self::ITEMS.len();
        }

        // -------------------------
        // Method::get() / post() ...
        // -------------------------
        pastey::paste! {
            impl $name {
                $(
                    #[inline]
                    pub fn [<$idents:lower>]() -> Self {
                        Self::$idents
                    }
                )*
            }
        }

        // -------------------------
        // Display => to_string
        // -------------------------
        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::$other(v) => write!(f, "{}: {}", stringify!($other), v),
                    $( Self::$idents => write!(f, "{}", stringify!($idents)), )*
                }
            }
        }

        // -------------------------
        // ParseError type: ParseMethodError
        // -------------------------
        pastey::paste! {
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct [<Parse $name Error>] {
                pub input: String,
            }

            impl ::core::fmt::Display for [<Parse $name Error>] {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    write!(f, "invalid {}: {}", stringify!($name), self.input)
                }
            }

            impl ::std::error::Error for [<Parse $name Error>] {}
        }

        // -------------------------
        // FromStr 宽松：未知 -> Other
        // -------------------------
        impl ::core::str::FromStr for $name {
            type Err = pastey::paste!{ [<Parse $name Error>] };

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let trimmed = s.trim();
                let lowered = trimmed.to_ascii_lowercase();

                match lowered.as_str() {
                    $(
                        x if x == stringify!($idents).to_ascii_lowercase() => Ok(Self::$idents),
                    )*
                    _ => Ok(Self::$other(trimmed.to_string())),
                }
            }
        }

        // -------------------------
        // From<String>：永不失败，未知 -> Other
        // 这样会触发 std 的 TryFrom blanket impl，不要再自己 impl TryFrom<String>
        // -------------------------
        impl From<String> for $name {
            fn from(value: String) -> Self {
                value.parse().unwrap_or_else(|_| Self::$other(value))
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                value.parse().unwrap_or_else(|_| Self::$other(value.to_string()))
            }
        }
    };
}

define_it!(
    /// nice to meet you
    #[derive(Eq, Hash, PartialEq,Debug,Clone)]
    pub enum Method {
        Other(String),
        GET,
        POST,
        PATCH,
        PUT,
        DETELE,
    }
);

