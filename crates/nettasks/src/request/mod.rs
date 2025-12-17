use pastey::paste;

use crate::r#const::HTTP_VERSION;

#[macro_export]
macro_rules! define_it {
    // macro! for enum
    (
        $( #[$attr_meta:meta] )*
        $v:vis enum $name:ident {
            $(
                $( #[$ident_attr_meta:meta] )*
                $idents:ident
            ),* $(,)?
        }
    ) => {
        $( #[$attr_meta] )*
        $v enum $name{
            $(
                $( #[$ident_attr_meta] )*
                $idents ,
            )*
        }

        impl $name {
            pub const ITEMS: &'static [Self] = &[
                $( Self::$idents, )*
            ];
            pub const ITEMS_COUNT: usize = Self::ITEMS.len();
        }

        paste! {
            macro_rules! [<with_variants_ $name>] {
                ($m:ident) => {
                    $m!($name; $( $idents ),*);
                };
            }
        }


        impl ::core::fmt::Display for $name{
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let value = match self {
                        $(
                            Self:: $idents =>  stringify!($idents),
                        )*
                    };
                    write!(f, "{}", value)
            }
        }
    };

    // macro! for struct
   (
        $( #[$attr_meta:meta] )*
        $v:vis struct $name:ident {
           $(
                $( #[$ident_attr_meta:meta] )*
                $vv:vis  $idents:ident: $idents_ty:ty = $default_val:expr
            ),* $(,)?
        }
    ) => {
        $( #[$attr_meta] )*
        $v struct $name{
            $(
                $( #[$ident_attr_meta] )*
                $idents: $idents_ty,
            )*
        }
        // TODO: impl default for $name

        impl ::core::fmt::Display for $name{
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let value = match self {
                        $(
                            Self:: $idents =>  stringify!($idents),
                        )*
                    };
                    write!(f, "{}", value)
            }
        }
    };
}

define_it!(
    /// nice to meet you
    #[derive(Debug)]
    pub enum ReqMethod {
        /// help
        PUT,
        GET,
        POST,
        DELETE,
    }
);

pub struct ReqBuilder {
    req: String,
}

macro_rules! impl_req_builder_methods {
    ($enum_ident:ident; $($variant:ident),* $(,)?) => {
        impl ReqBuilder {
            paste! {
                $(
                    pub fn [< $variant:lower >](self, route: &str) -> Self {
                        self.req_method($enum_ident::$variant, route)
                    }
                )*
            }
        }
    };
}

with_variants_ReqMethod!(impl_req_builder_methods);

impl ReqBuilder {
    pub fn new() -> Self {
        Self { req: String::new() }
    }
    /// Inner: Build the req method line like: GET /path HTTP/1.1
    fn __build_request_method(method: ReqMethod, route: &str, http_version: &str) -> String {
        format!("{} {} {}", method, route, http_version)
    }
    pub fn req_method(mut self, method: ReqMethod, route: &str) -> Self {
        self.req = Self::__build_request_method(method, route, HTTP_VERSION);
        self.req.push('\n');
        self
    }

    pub fn headers<I, S>(mut self, headers: I) -> Self
    where
        I: Iterator<Item = S>,
        S: AsRef<str>,
    {
        for h in headers {
            let h = h.as_ref();
            if !h.is_empty() {
                self.req.push_str(h);
                self.req.push('\n');
            }
        }
        self.req.push('\n');

        self
    }

    pub fn data(mut self, data: &str) -> Self {
        self.req.push_str(data);
        self.req.push('\n');

        self
    }

    pub fn build(self) -> String {
        self.req
    }
}
