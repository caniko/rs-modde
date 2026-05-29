//! Helpers that intentionally leak owned values into `'static` references,
//! used to promote runtime-loaded user game specs into the `'static` data the
//! game registry's plugin interfaces require.

pub(crate) fn str(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

pub(crate) fn slice<T>(items: Vec<T>) -> &'static [T] {
    Box::leak(items.into_boxed_slice())
}
