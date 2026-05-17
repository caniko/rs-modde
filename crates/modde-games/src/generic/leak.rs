pub(crate) fn str(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

pub(crate) fn slice<T>(items: Vec<T>) -> &'static [T] {
    Box::leak(items.into_boxed_slice())
}
