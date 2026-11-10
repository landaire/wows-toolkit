//! Lets a single-field newtype cross the JS boundary as its inner value, so
//! the generated TypeScript names the inner type.

/// Implements `nsh_core::element::JsValue` for `$name($inner)` by delegating
/// to `$inner`.
macro_rules! js_newtype {
    ($name:ty, $inner:ty) => {
        impl ::nsh_core::element::JsValue for $name {
            fn ts_type(types: &mut ::nsh_core::element::TsTypes) -> ::std::string::String {
                <$inner as ::nsh_core::element::JsValue>::ts_type(types)
            }

            fn from_js(
                value: ::nsh_core::element::JsIn<'_>,
            ) -> ::std::result::Result<Self, ::nsh_core::element::PropError> {
                <$inner as ::nsh_core::element::JsValue>::from_js(value).map(Self)
            }

            fn to_js<'a>(
                &self,
                host: &'a dyn ::nsh_core::element::JsHost,
            ) -> ::std::result::Result<::nsh_core::element::JsOwned<'a>, ::nsh_core::element::PropError> {
                <$inner as ::nsh_core::element::JsValue>::to_js(&self.0, host)
            }
        }
    };
}

pub(crate) use js_newtype;
